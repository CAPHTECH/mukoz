//! `native-routine` executor (docs/05 5.5, docs/08 8.4): the routine runs on the host CPU in a
//! sandboxed child process, at the same addresses and with the same registers and region
//! contents as the emulated executor, so that the two can be compared case by case (docs/06 6.8).
//!
//! What it observes: registers and flags at the sentinel return, region bytes after return,
//! crashes (signals) with the faulting address, and any system call (the seccomp filter kills
//! the process). What it cannot observe: individual memory accesses (only page-granular guards)
//! and the instruction trace.

use crate::emu::{Observation, Stop, isa_info, routine_setup, STACK_TOP, SENTINEL};
use crate::spec::Access;
use crate::host::{self, Exit, Sandbox};
use crate::image::Image;
use crate::plan::Case;
use crate::spec::{Binding, Isa};
use std::collections::BTreeMap;

const PAGE: u64 = 0x1000;
/// Trampoline context page (child-private), next to the sentinel page. Both lie in the range
/// Mukoz reserves for itself, so no segment or region can overlap them.
const CTX: u64 = 0x0deaf000;
const REGS: [&str; 16] = ["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"];
const SHM_HEADER: usize = 4096;
// Shared header words.
const H_STATUS: usize = 0; // 0 setup, 1 entered, 2 returned, 3 signal, 4 setup failed
const H_SIG: usize = 1;
const H_ADDR: usize = 2;
const H_RIP: usize = 3;
const H_ERR: usize = 4;
const H_SETUP: usize = 5;
const H_REGS: usize = 8;
const H_FLAGS: usize = 24;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
std::arch::global_asm!(
    ".globl mukoz_native_enter",
    "mukoz_native_enter:",
    "push rbx",
    "push rbp",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "mov qword ptr [0x0deaf000], rsp",
    "mov rsp, qword ptr [0x0deaf010]",
    "push qword ptr [0x0deaf018]",
    "popfq",
    "mov rbx, qword ptr [0x0deaf030]",
    "mov rcx, qword ptr [0x0deaf038]",
    "mov rdx, qword ptr [0x0deaf040]",
    "mov rsi, qword ptr [0x0deaf048]",
    "mov rdi, qword ptr [0x0deaf050]",
    "mov rbp, qword ptr [0x0deaf058]",
    "mov r8, qword ptr [0x0deaf068]",
    "mov r9, qword ptr [0x0deaf070]",
    "mov r10, qword ptr [0x0deaf078]",
    "mov r11, qword ptr [0x0deaf080]",
    "mov r12, qword ptr [0x0deaf088]",
    "mov r13, qword ptr [0x0deaf090]",
    "mov r14, qword ptr [0x0deaf098]",
    "mov r15, qword ptr [0x0deaf0a0]",
    "mov rax, qword ptr [0x0deaf028]",
    "jmp qword ptr [0x0deaf008]",
    ".globl mukoz_native_exit",
    "mukoz_native_exit:",
    "mov qword ptr [0x0deaf0a8], rax",
    "mov qword ptr [0x0deaf0b0], rbx",
    "mov qword ptr [0x0deaf0b8], rcx",
    "mov qword ptr [0x0deaf0c0], rdx",
    "mov qword ptr [0x0deaf0c8], rsi",
    "mov qword ptr [0x0deaf0d0], rdi",
    "mov qword ptr [0x0deaf0d8], rbp",
    "mov qword ptr [0x0deaf0e0], rsp",
    "mov qword ptr [0x0deaf0e8], r8",
    "mov qword ptr [0x0deaf0f0], r9",
    "mov qword ptr [0x0deaf0f8], r10",
    "mov qword ptr [0x0deaf100], r11",
    "mov qword ptr [0x0deaf108], r12",
    "mov qword ptr [0x0deaf110], r13",
    "mov qword ptr [0x0deaf118], r14",
    "mov qword ptr [0x0deaf120], r15",
    "pushfq",
    "pop rax",
    "mov qword ptr [0x0deaf128], rax",
    "mov rsp, qword ptr [0x0deaf000]",
    "cld",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "pop rbx",
    "ret",
);

// Context page layout (byte offsets).
#[allow(dead_code)] // written and read by the trampoline as [0x0deaf000]
const C_HOST_RSP: u64 = 0x00;
const C_ENTRY: u64 = 0x08;
const C_ENTRY_SP: u64 = 0x10;
const C_FLAGS_IN: u64 = 0x18;
const C_EXIT: u64 = 0x20;
const C_REGS_IN: u64 = 0x28;
const C_REGS_OUT: u64 = 0xa8;
const C_FLAGS_OUT: u64 = 0x128;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe extern "C" {
    fn mukoz_native_enter();
    fn mukoz_native_exit();
}

/// Can this host run `binding`'s target natively as a routine?
pub fn host_can_execute(binding: &Binding) -> Result<(), String> {
    if binding.target.is_process() {
        return Err("REQUIRED_CAPABILITY_UNAVAILABLE: native-routine runs routines; use native-process for process targets".into());
    }
    let host_isa = std::env::consts::ARCH;
    let want = match binding.target.isa {
        Isa::X86_64 => "x86_64",
        Isa::Aarch64 => "aarch64",
    };
    if host_isa != want {
        return Err(format!("HOST_CANNOT_EXECUTE_TARGET: host is {host_isa}, target ISA is {want}"));
    }
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(format!("REQUIRED_CAPABILITY_UNAVAILABLE: native-routine is implemented for linux-x86_64 only (host {}-{host_isa})", std::env::consts::OS));
    }
    Ok(())
}

struct Shm {
    ptr: *mut u8,
    len: usize,
}

impl Shm {
    fn new(len: usize) -> Option<Shm> {
        let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED | libc::MAP_ANONYMOUS, -1, 0) };
        if p == libc::MAP_FAILED { None } else { Some(Shm { ptr: p as *mut u8, len }) }
    }
    fn word(&self, i: usize) -> u64 {
        unsafe { std::ptr::read_volatile((self.ptr as *const u64).add(i)) }
    }
    fn bytes(&self, off: usize, n: usize) -> Vec<u8> {
        unsafe { std::slice::from_raw_parts(self.ptr.add(off), n).to_vec() }
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr as *mut _, self.len) };
    }
}

static SHM_ADDR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn shm_put(i: usize, v: u64) {
    let base = SHM_ADDR.load(std::sync::atomic::Ordering::Relaxed) as *mut u64;
    unsafe { std::ptr::write_volatile(base.add(i), v) };
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn exit_group(code: u64) -> ! {
    unsafe { std::arch::asm!("syscall", in("rax") 231u64, in("rdi") code, options(noreturn)) }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
extern "C" fn on_signal(sig: libc::c_int, info: *mut libc::siginfo_t, uc: *mut libc::c_void) {
    unsafe {
        let addr = (*info).si_addr() as u64;
        let ucx = uc as *const libc::ucontext_t;
        let rip = (*ucx).uc_mcontext.gregs[libc::REG_RIP as usize] as u64;
        let err = (*ucx).uc_mcontext.gregs[libc::REG_ERR as usize] as u64;
        shm_put(H_SIG, sig as u64);
        shm_put(H_ADDR, addr);
        shm_put(H_RIP, rip);
        shm_put(H_ERR, err);
        shm_put(H_STATUS, 3);
    }
    exit_group(0)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn setup_fail(step: u64) -> i32 {
    shm_put(H_SETUP, step);
    shm_put(H_STATUS, 4);
    0
}

/// Map `[lo, hi)` at exactly that address (never over an existing mapping).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe fn map_fixed(lo: u64, len: u64, prot: i32) -> bool {
    let p = unsafe { libc::mmap(lo as *mut _, len as usize, prot, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED_NOREPLACE, -1, 0) };
    p as u64 == lo
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn seccomp_exit_group_only() -> bool {
    // Allow exit_group only (x86_64 ABI); any other system call kills the process with SIGSYS.
    let f = |code: u16, jt: u8, jf: u8, k: u32| libc::sock_filter { code, jt, jf, k };
    let prog = [
        f(0x20, 0, 0, 4),           // ld [arch]
        f(0x15, 0, 3, 0xc000_003e), // jeq AUDIT_ARCH_X86_64
        f(0x20, 0, 0, 0),           // ld [nr]
        f(0x15, 0, 1, 231),         // jeq exit_group
        f(0x06, 0, 0, 0x7fff_0000), // ret ALLOW
        f(0x06, 0, 0, 0x8000_0000), // ret KILL_PROCESS
    ];
    let fp = libc::sock_fprog { len: prog.len() as u16, filter: prog.as_ptr() as *mut _ };
    unsafe {
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0
            && libc::syscall(libc::SYS_seccomp, 1 /* SECCOMP_SET_MODE_FILTER */, 0, &fp as *const _) == 0
    }
}

pub struct NativeRun {
    pub obs: Observation,
    pub applied_isolation: Vec<&'static str>,
}

/// Run one routine case natively. `wall_ms` bounds the child's wall time.
pub fn run(image: &Image, binding: &Binding, case: &Case, vary_placement: bool, wall_ms: u64) -> NativeRun {
    let blank = |stop: Stop| Observation {
        stop,
        regs_in: BTreeMap::new(),
        regs_out: BTreeMap::new(),
        flags_out: 0,
        regions_out: BTreeMap::new(),
        recent: Vec::new(),
        instructions: 0,
        process: None,
        monitors: BTreeMap::new(),
    };
    if let Err(e) = host_can_execute(binding) {
        return NativeRun { obs: blank(Stop::SetupError { error: e }), applied_isolation: vec![] };
    }
    if !image.monitors.is_empty() {
        return NativeRun { obs: blank(Stop::SetupError { error: "REQUIRED_CAPABILITY_UNAVAILABLE: boundary monitors need instruction hooks (emulated only)".into() }), applied_isolation: vec![] };
    }
    let rs = match routine_setup(binding, case, vary_placement) {
        Ok(r) => r,
        Err(e) => return NativeRun { obs: blank(Stop::SetupError { error: e }), applied_isolation: vec![] },
    };
    run_x86(image, binding, case, rs, wall_ms, blank)
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn run_x86(_: &Image, _: &Binding, _: &Case, _: crate::emu::RoutineSetup, _: u64, blank: impl Fn(Stop) -> Observation) -> NativeRun {
    NativeRun { obs: blank(Stop::SetupError { error: "REQUIRED_CAPABILITY_UNAVAILABLE".into() }), applied_isolation: vec![] }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run_x86(image: &Image, binding: &Binding, _case: &Case, rs: crate::emu::RoutineSetup, wall_ms: u64, blank: impl Fn(Stop) -> Observation) -> NativeRun {
    let data_len: usize = rs.regions.iter().map(|r| r.size as usize).sum();
    let Some(shm) = Shm::new(SHM_HEADER + data_len.max(1)) else {
        return NativeRun { obs: blank(Stop::SetupError { error: "mmap of the result area failed".into() }), applied_isolation: vec![] };
    };
    SHM_ADDR.store(shm.ptr as usize, std::sync::atomic::Ordering::Relaxed);
    let stack_size = binding.stack_bytes.max(PAGE).div_ceil(PAGE) * PAGE;
    let stack_lo = STACK_TOP - stack_size;
    let entry_sp = STACK_TOP - 64 - 8;
    let info = isa_info(Isa::X86_64);

    // Per-page protection of the image (union over segments sharing a page).
    let mut pages: BTreeMap<u64, i32> = BTreeMap::new();
    for s in &image.segments {
        let mut prot = 0;
        if s.read {
            prot |= libc::PROT_READ;
        }
        if s.write {
            prot |= libc::PROT_WRITE;
        }
        if s.exec {
            prot |= libc::PROT_EXEC | libc::PROT_READ;
        }
        let mut a = s.addr / PAGE * PAGE;
        while a < s.addr + s.mem_size.max(1) {
            *pages.entry(a).or_insert(0) |= prot;
            a += PAGE;
        }
    }
    let regs_in = rs.regs_in.clone();
    let regions = &rs.regions;
    let segments = &image.segments;
    let entry = image.entry;
    let cpu_s = wall_ms.div_ceil(1000) + 1;
    let root = host::make_trial_dir("routine");
    let sb = Sandbox { as_bytes: None, ..Sandbox::trial(Some(root.clone()), cpu_s) };
    let applied = sb.applied();
    let exit = host::run(&sb, wall_ms, &[], move || unsafe {
        // 1. Image: map writable, copy, then apply the final protection (W^X).
        for &a in pages.keys() {
            if !map_fixed(a, PAGE, libc::PROT_READ | libc::PROT_WRITE) {
                return setup_fail(1);
            }
        }
        for s in segments {
            std::ptr::copy_nonoverlapping(s.data.as_ptr(), s.addr as *mut u8, s.data.len());
        }
        for (&a, &prot) in &pages {
            let prot = if prot & libc::PROT_EXEC != 0 { prot & !libc::PROT_WRITE } else { prot };
            if libc::mprotect(a as *mut _, PAGE as usize, prot) != 0 {
                return setup_fail(2);
            }
        }
        // 2. Stack with the sentinel return address.
        if !map_fixed(stack_lo, stack_size, libc::PROT_READ | libc::PROT_WRITE) {
            return setup_fail(3);
        }
        *(entry_sp as *mut u64) = SENTINEL;
        // 3. Regions, each followed by an inaccessible guard page.
        for r in regions {
            let body = r.map_size - PAGE;
            if !map_fixed(r.map_base, body, libc::PROT_READ | libc::PROT_WRITE) || !map_fixed(r.map_base + body, PAGE, libc::PROT_NONE) {
                return setup_fail(4);
            }
            std::ptr::copy_nonoverlapping(r.init.as_ptr(), r.addr as *mut u8, r.init.len());
            let prot = match r.access {
                Access::R => libc::PROT_READ,
                Access::W | Access::Rw => libc::PROT_READ | libc::PROT_WRITE,
            };
            if libc::mprotect(r.map_base as *mut _, body as usize, prot) != 0 {
                return setup_fail(5);
            }
        }
        // 4. Sentinel page: `jmp qword ptr [CTX + C_EXIT]`; context page.
        if !map_fixed(SENTINEL, PAGE, libc::PROT_READ | libc::PROT_WRITE) || !map_fixed(CTX, PAGE, libc::PROT_READ | libc::PROT_WRITE) {
            return setup_fail(6);
        }
        let d = ((CTX + C_EXIT) as u32).to_le_bytes();
        let jmp = [0xff, 0x24, 0x25, d[0], d[1], d[2], d[3]];
        std::ptr::copy_nonoverlapping(jmp.as_ptr(), SENTINEL as *mut u8, jmp.len());
        if libc::mprotect(SENTINEL as *mut _, PAGE as usize, libc::PROT_READ | libc::PROT_EXEC) != 0 {
            return setup_fail(7);
        }
        let put = |off: u64, v: u64| std::ptr::write_volatile((CTX + off) as *mut u64, v);
        put(C_ENTRY, entry);
        put(C_ENTRY_SP, entry_sp);
        put(C_FLAGS_IN, 0x202);
        put(C_EXIT, mukoz_native_exit as *const () as u64);
        for (i, r) in REGS.iter().enumerate() {
            put(C_REGS_IN + 8 * i as u64, regs_in.get(*r).copied().unwrap_or(0));
        }
        // 5. Signals on an alternate stack; then the syscall filter; then enter.
        let alt = libc::mmap(std::ptr::null_mut(), 1 << 16, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0);
        if alt == libc::MAP_FAILED {
            return setup_fail(8);
        }
        let ss = libc::stack_t { ss_sp: alt, ss_flags: 0, ss_size: 1 << 16 };
        if libc::sigaltstack(&ss, std::ptr::null_mut()) != 0 {
            return setup_fail(9);
        }
        for sig in [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGTRAP] {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_signal as *const () as usize;
            sa.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
            libc::sigemptyset(&mut sa.sa_mask);
            if libc::sigaction(sig, &sa, std::ptr::null_mut()) != 0 {
                return setup_fail(10);
            }
        }
        if !seccomp_exit_group_only() {
            return setup_fail(11);
        }
        shm_put(H_STATUS, 1);
        mukoz_native_enter();
        // Back from the sentinel: record registers and regions, then leave.
        for i in 0..16 {
            shm_put(H_REGS + i, std::ptr::read_volatile((CTX + C_REGS_OUT + 8 * i as u64) as *const u64));
        }
        shm_put(H_FLAGS, std::ptr::read_volatile((CTX + C_FLAGS_OUT) as *const u64));
        let mut off = SHM_HEADER;
        let base = SHM_ADDR.load(std::sync::atomic::Ordering::Relaxed) as *mut u8;
        for r in regions {
            std::ptr::copy_nonoverlapping(r.addr as *const u8, base.add(off), r.size as usize);
            off += r.size as usize;
        }
        shm_put(H_STATUS, 2);
        exit_group(0)
    });
    host::remove_dir(&root);

    let loc = |a: u64| image.locate(a);
    let status = shm.word(H_STATUS);
    let stop = match exit {
        Exit::TimedOut | Exit::Killed => Stop::Timeout { instructions: 0 },
        Exit::Signal(s) if s == libc::SIGSYS => Stop::ForbiddenEffect { pc_offset: "unknown (native)".into(), effect: "system call (killed by the seccomp filter)".into() },
        Exit::Signal(s) if s == libc::SIGXCPU || s == libc::SIGKILL => Stop::Timeout { instructions: 0 },
        Exit::Signal(s) => Stop::EngineError { error: format!("native child killed by signal {s}") },
        Exit::Code(125) => Stop::SetupError { error: "the sandbox could not be entered".into() },
        Exit::Code(0) if status == 4 => Stop::SetupError { error: format!("native setup step {} failed", shm.word(H_SETUP)) },
        Exit::Code(0) if status == 1 => Stop::ForbiddenEffect { pc_offset: "unknown (native)".into(), effect: "system call exit_group".into() },
        Exit::Code(0) if status == 2 => {
            let sp = shm.word(H_REGS + 7);
            if sp == entry_sp + 8 { Stop::Returned } else { Stop::BadReturn { sp: format!("0x{sp:x}"), expected_sp: format!("0x{:x}", entry_sp + 8) } }
        }
        Exit::Code(0) if status == 3 => {
            let (sig, addr, rip, err) = (shm.word(H_SIG) as i32, shm.word(H_ADDR), shm.word(H_RIP), shm.word(H_ERR));
            match sig {
                s if s == libc::SIGSEGV || s == libc::SIGBUS => {
                    if err & 0x10 != 0 || addr == rip {
                        Stop::LeftCode { from_offset: None, target: loc(rip) }
                    } else {
                        Stop::MemoryViolation {
                            pc_offset: loc(rip),
                            access: if err & 2 != 0 { "write".into() } else { "read".into() },
                            address: format!("0x{addr:x}"),
                            size: 0,
                            detail: "native-routine: the access hit an unmapped or protected page (page granularity; the width is not observed)".into(),
                        }
                    }
                }
                s if s == libc::SIGILL => Stop::InvalidInstruction { pc_offset: loc(rip) },
                s if s == libc::SIGFPE => Stop::ForbiddenEffect { pc_offset: loc(rip), effect: "interrupt/exception 0 (#DE, divide error)".into() },
                s if s == libc::SIGTRAP => Stop::ForbiddenEffect { pc_offset: loc(rip), effect: "interrupt/exception 3 (breakpoint)".into() },
                s => Stop::EngineError { error: format!("signal {s} at {}", loc(rip)) },
            }
        }
        Exit::Code(c) => Stop::EngineError { error: format!("native child exited with {c} (status word {status})") },
    };
    let mut regs_out = BTreeMap::new();
    let mut flags_out = 0;
    let mut regions_out = BTreeMap::new();
    if status == 2 {
        for (i, r) in REGS.iter().enumerate() {
            if info.gprs.contains(r) {
                regs_out.insert(r.to_string(), shm.word(H_REGS + i));
            }
        }
        flags_out = shm.word(H_FLAGS);
        let mut off = SHM_HEADER;
        for r in &rs.regions {
            regions_out.insert(r.name.clone(), shm.bytes(off, r.size as usize));
            off += r.size as usize;
        }
    }
    NativeRun { obs: Observation { stop, regs_in: rs.regs_in, regs_out, flags_out, regions_out, recent: Vec::new(), instructions: 0, process: None, monitors: BTreeMap::new() }, applied_isolation: applied }
}
