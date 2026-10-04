//! Emulated routine executor (docs/05 5.3-5.5) on Unicorn.
//!
//! One fresh engine instance per case: re-using an instance after rewriting
//! code was observed to execute stale translated blocks (docs/devlog.md).

use crate::expr::{self, EvalCtx, Ty, ValEnv, Value};
use crate::plan::{Case, SplitMix64};
use crate::spec::{Access, Binding, Isa};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use unicorn_engine::unicorn_const::{Arch, HookType, MemType, Mode, Permission, uc_error};
use unicorn_engine::{InsnSysX86, RegisterARM64, RegisterX86, Unicorn};

pub const ENGINE: &str = "unicorn-engine 2.1.1 (bundled C, crate pin)";
pub const CODE_BASE: u64 = 0x0010_0000;
pub const STACK_TOP: u64 = 0x7fff_0000;
pub const REGION_BASE: u64 = 0x2000_0000;
pub const REGION_STRIDE: u64 = 0x0100_0000;
pub const SENTINEL: u64 = 0x0dea_d000;
const PAGE: u64 = 0x1000;
const RING: usize = 64;

fn page_up(n: u64) -> u64 {
    n.div_ceil(PAGE) * PAGE
}

// ---------------------------------------------------------------- ISA tables

pub struct IsaInfo {
    pub args: &'static [&'static str],
    pub results: &'static [&'static str],
    pub callee_saved: &'static [&'static str],
    pub gprs: &'static [&'static str],
}

pub fn isa_info(isa: Isa) -> IsaInfo {
    match isa {
        Isa::X86_64 => IsaInfo {
            args: &["rdi", "rsi", "rdx", "rcx", "r8", "r9"],
            results: &["rax", "rdx"],
            callee_saved: &["rbx", "rbp", "r12", "r13", "r14", "r15"],
            gprs: &["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"],
        },
        Isa::Aarch64 => IsaInfo {
            args: &["x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7"],
            results: &["x0", "x1"],
            callee_saved: &["x19", "x20", "x21", "x22", "x23", "x24", "x25", "x26", "x27", "x28", "x29"],
            gprs: &[
                "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", "x13", "x14", "x15", "x16",
                "x17", "x18", "x19", "x20", "x21", "x22", "x23", "x24", "x25", "x26", "x27", "x28", "x29",
            ],
        },
    }
}

pub fn is_arg_or_result_reg(isa: Isa, r: &str) -> bool {
    let i = isa_info(isa);
    i.args.contains(&r) || i.results.contains(&r)
}

fn reg_id(isa: Isa, name: &str) -> i32 {
    match isa {
        Isa::X86_64 => {
            let r = match name {
                "rax" => RegisterX86::RAX,
                "rbx" => RegisterX86::RBX,
                "rcx" => RegisterX86::RCX,
                "rdx" => RegisterX86::RDX,
                "rsi" => RegisterX86::RSI,
                "rdi" => RegisterX86::RDI,
                "rbp" => RegisterX86::RBP,
                "rsp" => RegisterX86::RSP,
                "r8" => RegisterX86::R8,
                "r9" => RegisterX86::R9,
                "r10" => RegisterX86::R10,
                "r11" => RegisterX86::R11,
                "r12" => RegisterX86::R12,
                "r13" => RegisterX86::R13,
                "r14" => RegisterX86::R14,
                "r15" => RegisterX86::R15,
                "rflags" => RegisterX86::RFLAGS,
                _ => panic!("unknown x86 register {name}"),
            };
            r.into()
        }
        Isa::Aarch64 => {
            let r = match name {
                "sp" => RegisterARM64::SP,
                "x29" => RegisterARM64::X29,
                "x30" => RegisterARM64::X30,
                _ => {
                    let n: i32 = name.strip_prefix('x').and_then(|n| n.parse().ok()).expect("aarch64 register");
                    let x0: i32 = RegisterARM64::X0.into();
                    // X0..X28 are contiguous in Unicorn's enum.
                    assert!(n <= 28);
                    return x0 + n;
                }
            };
            r.into()
        }
    }
}

// ---------------------------------------------------------------- results

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stop {
    Returned,
    /// Reached the return sentinel with the wrong stack pointer.
    BadReturn { sp: String, expected_sp: String },
    /// Memory access outside the allowed regions.
    MemoryViolation { pc_offset: String, access: String, address: String, size: usize, detail: String },
    /// Control left the code region.
    LeftCode { from_offset: Option<String>, target: String },
    InvalidInstruction { pc_offset: String },
    ForbiddenEffect { pc_offset: String, effect: String },
    ReservedRegisterUsed { pc_offset: String, register: String },
    BudgetExhausted { instructions: u64 },
    Timeout { instructions: u64 },
    EngineError { error: String },
    SetupError { error: String },
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Insn {
    pub offset: String,
    pub bytes: String,
}

#[derive(Clone, Debug)]
pub struct Observation {
    pub stop: Stop,
    pub regs_in: BTreeMap<String, u64>,
    pub regs_out: BTreeMap<String, u64>,
    pub flags_out: u64,
    pub regions_out: BTreeMap<String, Vec<u8>>,
    pub recent: Vec<Insn>,
    pub instructions: u64,
}

pub struct PlacedRegion {
    pub name: String,
    pub addr: u64,
    pub size: u64,
    pub access: Access,
}

// ---------------------------------------------------------------- execution

struct HookState {
    stop: Option<Stop>,
    count: u64,
    ring: Vec<(u64, u32)>,
}

pub struct Executor<'a> {
    pub code: &'a [u8],
    pub binding: &'a Binding,
    pub insn_limit: u64,
    pub timeout_ms: u64,
}

fn rel(addr: u64) -> String {
    if (CODE_BASE..CODE_BASE + 0x1000_0000).contains(&addr) {
        format!("0x{:x}", addr - CODE_BASE)
    } else {
        format!("abs:0x{addr:x}")
    }
}

impl Executor<'_> {
    fn code_ranges(&self) -> Vec<(u64, u64)> {
        if self.binding.code_regions.is_empty() {
            vec![(CODE_BASE, CODE_BASE + self.code.len() as u64)]
        } else {
            self.binding.code_regions.iter().map(|r| (CODE_BASE + r.offset, CODE_BASE + r.offset + r.size)).collect()
        }
    }

    pub fn run(&self, case: &Case) -> Observation {
        match self.run_inner(case) {
            Ok(o) => o,
            Err(e) => Observation {
                stop: Stop::SetupError { error: e },
                regs_in: BTreeMap::new(),
                regs_out: BTreeMap::new(),
                flags_out: 0,
                regions_out: BTreeMap::new(),
                recent: Vec::new(),
                instructions: 0,
            },
        }
    }

    fn run_inner(&self, case: &Case) -> Result<Observation, String> {
        let isa = self.binding.target.isa;
        let info = isa_info(isa);
        let apple = self.binding.target.abi == "apple-arm64";
        let ue = |e: uc_error| format!("engine: {e:?}");
        let mut uc = match isa {
            Isa::X86_64 => Unicorn::new(Arch::X86, Mode::MODE_64),
            Isa::Aarch64 => Unicorn::new(Arch::ARM64, Mode::ARM),
        }
        .map_err(ue)?;

        // Code: whole file, read+exec.
        let code_len = self.code.len() as u64;
        if code_len == 0 {
            return Err("artifact is empty".into());
        }
        if self.binding.entry_offset >= code_len {
            return Err(format!("entry offset {} is outside the artifact ({} bytes)", self.binding.entry_offset, code_len));
        }
        for r in &self.binding.code_regions {
            if r.offset.checked_add(r.size).is_none_or(|e| e > code_len) {
                return Err(format!("BINDING_MISMATCH: code region {}+{} exceeds the artifact ({code_len} bytes)", r.offset, r.size));
            }
        }
        uc.mem_map(CODE_BASE, page_up(code_len) as usize, Permission::READ | Permission::EXEC).map_err(ue)?;
        uc.mem_write(CODE_BASE, self.code).map_err(ue)?;

        // Stack.
        let stack_size = page_up(self.binding.stack_bytes.max(PAGE));
        let stack_lo = STACK_TOP - stack_size;
        uc.mem_map(stack_lo, stack_size as usize, Permission::READ | Permission::WRITE).map_err(ue)?;

        // Regions: values from the case, addresses fixed per region index.
        let empty = BTreeMap::new();
        let mut addrs: BTreeMap<String, u64> = BTreeMap::new();
        let mut placed: Vec<PlacedRegion> = Vec::new();
        let mut filler = SplitMix64::new(case.filler_seed);
        let mut inits: Vec<(u64, Vec<u8>)> = Vec::new();
        for (i, r) in self.binding.regions.iter().enumerate() {
            let cx = EvalCtx { vars: &case.values, region_addrs: &empty };
            let size = match expr::eval(&r.size, &cx) {
                Ok(Value::Bv(_, n)) => n,
                Ok(v) => return Err(format!("region {} size is {}", r.name, v.ty())),
                Err(m) => return Err(format!("region {} size: {m}", r.name)),
            };
            if size > 1 << 20 {
                return Err(format!("region {} size {size} exceeds 1 MiB", r.name));
            }
            let init = match &r.init {
                Some(e) => match expr::eval(e, &cx).map_err(|m| format!("region {} init: {m}", r.name))? {
                    Value::Bytes(b) => b,
                    Value::Bv(w, x) => x.to_le_bytes()[..(w / 8) as usize].to_vec(),
                    Value::Bool(b) => vec![b as u8],
                },
                // Uninitialized: random bytes, never zeros.
                None => (0..size).map(|_| filler.next() as u8).collect(),
            };
            if init.len() as u64 != size {
                return Err(format!("BINDING_MISMATCH: region {} init has {} bytes but size is {size}", r.name, init.len()));
            }
            let map_base = REGION_BASE + i as u64 * REGION_STRIDE;
            let map_size = page_up(size.max(1)) + PAGE;
            // Place the data so that it ends 16-byte aligned near the end of the mapping.
            let addr = map_base + map_size - PAGE - ((size + 15) / 16 * 16);
            uc.mem_map(map_base, map_size as usize, Permission::READ | Permission::WRITE).map_err(ue)?;
            inits.push((addr, init));
            addrs.insert(r.name.clone(), addr);
            placed.push(PlacedRegion { name: r.name.clone(), addr, size, access: r.access });
        }
        for (addr, b) in &inits {
            uc.mem_write(*addr, b).map_err(ue)?;
        }

        // Registers: filler for everything, then arguments.
        let mut regs_in: BTreeMap<String, u64> = BTreeMap::new();
        for g in info.gprs {
            regs_in.insert(g.to_string(), filler.next());
        }
        let cx = EvalCtx { vars: &case.values, region_addrs: &addrs };
        for (reg, e) in &self.binding.arguments {
            let v = match expr::eval(e, &cx).map_err(|m| format!("argument {reg}: {m}"))? {
                // Narrow arguments: the ABI leaves upper bits unspecified, so fill them.
                Value::Bv(w, x) if w < 64 => x | (filler.next() & !expr::mask(w)),
                Value::Bv(_, x) => x,
                Value::Bool(b) => b as u64,
                Value::Bytes(_) => return Err(format!("argument {reg} evaluated to bytes")),
            };
            regs_in.insert(reg.clone(), v);
        }
        let entry_sp;
        match isa {
            Isa::X86_64 => {
                // Call just happened: [rsp] = return address, rsp ≡ 8 (mod 16).
                entry_sp = STACK_TOP - 64 - 8;
                uc.mem_write(entry_sp, &SENTINEL.to_le_bytes()).map_err(ue)?;
                uc.reg_write(RegisterX86::RSP, entry_sp).map_err(ue)?;
                uc.reg_write(RegisterX86::RFLAGS, 0x2).map_err(ue)?;
            }
            Isa::Aarch64 => {
                entry_sp = STACK_TOP - 64;
                // A valid (zeroed) caller frame record for x29.
                regs_in.insert("x29".into(), entry_sp + 16);
                uc.mem_write(entry_sp + 16, &[0u8; 16]).map_err(ue)?;
                uc.reg_write(RegisterARM64::SP, entry_sp).map_err(ue)?;
                uc.reg_write(RegisterARM64::X30, SENTINEL).map_err(ue)?;
            }
        }
        for (r, v) in &regs_in {
            uc.reg_write(reg_id(isa, r), *v).map_err(ue)?;
        }

        // Allowed data accesses: (lo, hi, read, write, label).
        let stack_hi = match isa {
            Isa::X86_64 => entry_sp + 8,
            Isa::Aarch64 => entry_sp,
        };
        let mut allowed: Vec<(u64, u64, bool, bool, String)> = Vec::new();
        allowed.push((CODE_BASE, CODE_BASE + code_len, true, false, "code (read-only)".into()));
        allowed.push((stack_lo, stack_hi, true, true, "stack (below the entry stack pointer)".into()));
        for p in &placed {
            let (r, w) = match p.access {
                Access::R => (true, false),
                Access::W => (false, true),
                Access::Rw => (true, true),
            };
            allowed.push((p.addr, p.addr + p.size, r, w, format!("region `{}`", p.name)));
        }
        let allowed = Rc::new(allowed);

        let st = Rc::new(RefCell::new(HookState { stop: None, count: 0, ring: Vec::with_capacity(RING) }));
        let code_ranges = self.code_ranges();

        // Instruction hook: ring buffer, budget accounting, code range check.
        {
            let st = st.clone();
            let x18_in = regs_in.get("x18").copied();
            let x18 = if apple { Some(reg_id(isa, "x18")) } else { None };
            uc.add_code_hook(0, u64::MAX, move |uc, addr, size| {
                let mut s = st.borrow_mut();
                if s.stop.is_some() {
                    return;
                }
                // Unicorn reports an undecodable instruction with this size marker.
                if size == 0xf1f1_f1f1 {
                    s.stop = Some(Stop::InvalidInstruction { pc_offset: rel(addr) });
                    drop(s);
                    let _ = uc.emu_stop();
                    return;
                }
                if !code_ranges.iter().any(|(lo, hi)| addr >= *lo && addr + size as u64 <= *hi) {
                    let from = s.ring.last().map(|(a, _)| rel(*a));
                    s.stop = Some(Stop::LeftCode { from_offset: from, target: rel(addr) });
                    drop(s);
                    let _ = uc.emu_stop();
                    return;
                }
                if let (Some(id), Some(v0)) = (x18, x18_in) {
                    if uc.reg_read(id).unwrap_or(v0) != v0 {
                        let at = s.ring.last().map(|(a, _)| rel(*a)).unwrap_or_default();
                        s.stop = Some(Stop::ReservedRegisterUsed { pc_offset: at, register: "x18".into() });
                        drop(s);
                        let _ = uc.emu_stop();
                        return;
                    }
                }
                s.count += 1;
                if s.ring.len() == RING {
                    s.ring.remove(0);
                }
                s.ring.push((addr, size));
            })
            .map_err(ue)?;
        }
        // Data access monitor.
        {
            let st = st.clone();
            let allowed = allowed.clone();
            uc.add_mem_hook(HookType::MEM_READ | HookType::MEM_WRITE, 0, u64::MAX, move |uc, t, addr, size, _v| {
                let write = matches!(t, MemType::WRITE);
                let end = addr.checked_add(size as u64);
                let ok = end.is_some_and(|end| {
                    allowed.iter().any(|(lo, hi, r, w, _)| addr >= *lo && end <= *hi && if write { *w } else { *r })
                });
                if !ok {
                    let mut s = st.borrow_mut();
                    if s.stop.is_none() {
                        let pc = uc.pc_read().unwrap_or(0);
                        s.stop = Some(Stop::MemoryViolation {
                            pc_offset: rel(pc),
                            access: if write { "write".into() } else { "read".into() },
                            address: format!("0x{addr:x}"),
                            size,
                            detail: describe_access(addr, size, write, &allowed),
                        });
                    }
                    drop(s);
                    let _ = uc.emu_stop();
                }
                true
            })
            .map_err(ue)?;
        }
        // Unmapped / protected accesses (the engine faults; record where).
        {
            let st = st.clone();
            let allowed = allowed.clone();
            uc.add_mem_hook(HookType::MEM_INVALID, 0, u64::MAX, move |uc, t, addr, size, _v| {
                let mut s = st.borrow_mut();
                if s.stop.is_none() {
                    let pc = uc.pc_read().unwrap_or(0);
                    s.stop = Some(match t {
                        MemType::FETCH_UNMAPPED | MemType::FETCH_PROT => {
                            let from = s.ring.last().map(|(a, _)| rel(*a));
                            Stop::LeftCode { from_offset: from, target: rel(addr) }
                        }
                        _ => {
                            let write = matches!(t, MemType::WRITE_UNMAPPED | MemType::WRITE_PROT);
                            Stop::MemoryViolation {
                                pc_offset: rel(pc),
                                access: if write { "write".into() } else { "read".into() },
                                address: format!("0x{addr:x}"),
                                size,
                                detail: describe_access(addr, size, write, &allowed),
                            }
                        }
                    });
                }
                false
            })
            .map_err(ue)?;
        }
        // Any trap to an OS is a forbidden effect for a routine.
        {
            let st = st.clone();
            uc.add_intr_hook(move |uc, intno| {
                let mut s = st.borrow_mut();
                if s.stop.is_none() {
                    let pc = s.ring.last().map(|(a, _)| *a).unwrap_or_else(|| uc.pc_read().unwrap_or(0));
                    s.stop = Some(Stop::ForbiddenEffect { pc_offset: rel(pc), effect: format!("interrupt/exception {intno} (e.g. svc/int/brk)") });
                }
                drop(s);
                let _ = uc.emu_stop();
            })
            .map_err(ue)?;
        }
        if isa == Isa::X86_64 {
            for (kind, name) in [(InsnSysX86::SYSCALL, "syscall"), (InsnSysX86::SYSENTER, "sysenter")] {
                let st = st.clone();
                uc.add_insn_sys_hook(kind, 0, u64::MAX, move |uc| {
                    let mut s = st.borrow_mut();
                    if s.stop.is_none() {
                        let pc = s.ring.last().map(|(a, _)| *a).unwrap_or(0);
                        s.stop = Some(Stop::ForbiddenEffect { pc_offset: rel(pc), effect: name.into() });
                    }
                    drop(s);
                    let _ = uc.emu_stop();
                })
                .map_err(ue)?;
            }
        }

        let started = std::time::Instant::now();
        let res = uc.emu_start(CODE_BASE + self.binding.entry_offset, SENTINEL, self.timeout_ms * 1000, self.insn_limit as usize);
        let elapsed = started.elapsed();

        let pc = uc.pc_read().unwrap_or(0);
        let sp = match isa {
            Isa::X86_64 => uc.reg_read(RegisterX86::RSP).unwrap_or(0),
            Isa::Aarch64 => uc.reg_read(RegisterARM64::SP).unwrap_or(0),
        };
        let s = st.borrow();
        let stop = if let Some(stop) = s.stop.clone() {
            stop
        } else {
            match res {
                Ok(()) if pc == SENTINEL => {
                    let expected = match isa {
                        Isa::X86_64 => entry_sp + 8,
                        Isa::Aarch64 => entry_sp,
                    };
                    if sp == expected {
                        Stop::Returned
                    } else {
                        Stop::BadReturn { sp: format!("0x{sp:x}"), expected_sp: format!("0x{expected:x}") }
                    }
                }
                Ok(()) if s.count >= self.insn_limit => Stop::BudgetExhausted { instructions: s.count },
                Ok(()) if elapsed.as_millis() as u64 >= self.timeout_ms => Stop::Timeout { instructions: s.count },
                Ok(()) => Stop::EngineError { error: format!("engine stopped at pc {} without a recorded reason", rel(pc)) },
                Err(uc_error::INSN_INVALID) => {
                    let at = if (CODE_BASE..CODE_BASE + code_len).contains(&pc) { pc } else { s.ring.last().map(|(a, _)| *a).unwrap_or(pc) };
                    Stop::InvalidInstruction { pc_offset: rel(at) }
                }
                Err(e) => Stop::EngineError { error: format!("{e:?} at pc {}", rel(pc)) },
            }
        };

        let mut regs_out = BTreeMap::new();
        for g in info.gprs {
            regs_out.insert(g.to_string(), uc.reg_read(reg_id(isa, g)).unwrap_or(0));
        }
        if isa == Isa::Aarch64 {
            regs_out.insert("x30".into(), uc.reg_read(RegisterARM64::X30).unwrap_or(0));
        }
        let flags_out = match isa {
            Isa::X86_64 => uc.reg_read(RegisterX86::RFLAGS).unwrap_or(0),
            Isa::Aarch64 => 0,
        };
        let mut regions_out = BTreeMap::new();
        for p in &placed {
            regions_out.insert(p.name.clone(), uc.mem_read_as_vec(p.addr, p.size as usize).unwrap_or_default());
        }
        let recent = s
            .ring
            .iter()
            .map(|(a, n)| {
                let off = a.wrapping_sub(CODE_BASE) as usize;
                let bytes = self.code.get(off..off + *n as usize).map(expr::hex).unwrap_or_default();
                Insn { offset: rel(*a), bytes }
            })
            .collect();
        Ok(Observation { stop, regs_in, regs_out, flags_out, regions_out, recent, instructions: s.count })
    }
}

fn describe_access(addr: u64, size: usize, write: bool, allowed: &[(u64, u64, bool, bool, String)]) -> String {
    let kind = if write { "write" } else { "read" };
    let end = addr.saturating_add(size as u64);
    for (lo, hi, r, w, label) in allowed {
        let overlaps = addr < *hi && end > *lo;
        if overlaps {
            if addr >= *lo && end <= *hi {
                return format!("{kind} of {size} bytes inside {label}, which does not allow {kind}s");
            }
            if addr < *lo {
                return format!("{kind} of {size} bytes starts {} bytes before {label} [0x{lo:x}, 0x{hi:x})", lo - addr);
            }
            return format!("{kind} of {size} bytes runs {} bytes past the end of {label} [0x{lo:x}, 0x{hi:x})", end - hi);
        }
        let _ = (r, w);
    }
    // Nearest allowed range for orientation.
    let nearest = allowed
        .iter()
        .min_by_key(|(lo, hi, ..)| if addr >= *hi { addr - hi } else { lo.saturating_sub(addr) })
        .map(|(lo, hi, _, _, l)| {
            if addr >= *hi {
                format!("; {} bytes past the end of {l} [0x{lo:x}, 0x{hi:x})", addr - hi)
            } else {
                format!("; {} bytes before {l} [0x{lo:x}, 0x{hi:x})", lo - addr)
            }
        })
        .unwrap_or_default();
    format!("{kind} of {size} bytes at 0x{addr:x} is outside every allowed range{nearest}")
}

/// Values the contract sees after a returned execution.
pub fn post_values(binding: &Binding, case: &Case, obs: &Observation, results_ty: &BTreeMap<String, Ty>, state_ty: &BTreeMap<String, Ty>) -> Result<ValEnv, String> {
    let mut env = case.values.clone();
    for (name, reg) in &binding.results {
        let raw = *obs.regs_out.get(reg).ok_or_else(|| format!("register {reg} not observed"))?;
        let ty = results_ty.get(name).copied().unwrap_or(Ty::Bv(64));
        let v = match ty {
            Ty::Bv(w) => Value::bv(w, raw),
            Ty::Bool => Value::Bool(raw & 1 == 1),
            Ty::Bytes => return Err("bytes results are not supported".into()),
        };
        env.insert(format!("result.{name}"), v);
    }
    for r in &binding.regions {
        if let Some(st) = &r.observe_state {
            let bytes = obs.regions_out.get(&r.name).cloned().unwrap_or_default();
            let v = match state_ty.get(st).copied().unwrap_or(Ty::Bytes) {
                Ty::Bytes => Value::Bytes(bytes),
                Ty::Bv(w) => {
                    let n = (w / 8) as usize;
                    if bytes.len() != n {
                        return Err(format!("state {st} is bv{w} but region {} has {} bytes", r.name, bytes.len()));
                    }
                    let mut buf = [0u8; 8];
                    buf[..n].copy_from_slice(&bytes);
                    Value::Bv(w, u64::from_le_bytes(buf))
                }
                Ty::Bool => Value::Bool(bytes.first().copied().unwrap_or(0) != 0),
            };
            env.insert(format!("after.{st}"), v);
        }
    }
    Ok(env)
}
