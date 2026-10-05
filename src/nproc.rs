//! `native-process` executor (docs/05 5.5, docs/08 8.4): the program runs on the host kernel in
//! a sandbox (user/network/PID namespaces, chroot into a directory holding only the declared
//! files, rlimits, empty environment), started from a sealed memfd with execveat so the path is
//! never resolved again. Raw images are wrapped into a static ELF with the same segments.
//!
//! What it observes: stdout/stderr bytes, the exit status or signal, and the declared files after
//! exit. What it does not observe: registers, memory accesses, system calls (docs/06 6.3).

use crate::emu::{Observation, ProcOut, Stop};
use crate::expr::{self, EvalCtx, Value};
use crate::host::{self, Exit, Sandbox};
use crate::image::{Image, Segment, DATA_BASE};
use crate::plan::Case;
use crate::spec::{Binding, Contract, Format, Isa};
use std::collections::BTreeMap;
use std::ffi::CString;

const DEFAULT_CAP: usize = 1 << 16;

pub fn host_can_execute(binding: &Binding) -> Result<(), String> {
    if !binding.target.is_process() {
        return Err("REQUIRED_CAPABILITY_UNAVAILABLE: native-process runs processes; use native-routine for routine targets".into());
    }
    let host_isa = std::env::consts::ARCH;
    let want = match binding.target.isa {
        Isa::X86_64 => "x86_64",
        Isa::Aarch64 => "aarch64",
    };
    if host_isa != want {
        return Err(format!("HOST_CANNOT_EXECUTE_TARGET: host is {host_isa}, target ISA is {want}"));
    }
    if binding.target.os != "linux" || std::env::consts::OS != "linux" {
        return Err(format!("HOST_CANNOT_EXECUTE_TARGET: target OS {} on a {} host", binding.target.os, std::env::consts::OS));
    }
    Ok(())
}

/// A static ET_EXEC holding `segs` at their addresses (the kernel loads it like any ELF).
pub fn wrap_elf(isa: Isa, entry: u64, segs: &[Segment]) -> Vec<u8> {
    const PG: u64 = 0x1000;
    let ph_off = 64u64;
    let n = segs.len() as u64;
    let mut off = (ph_off + 56 * n).div_ceil(PG) * PG;
    let mut out = vec![0u8; off as usize];
    let mut ph = Vec::new();
    for s in segs {
        // p_offset ≡ p_vaddr (mod page)
        let want = s.addr % PG;
        if off % PG != want {
            off = off.div_ceil(PG) * PG + want;
        }
        out.resize(off as usize, 0);
        out.extend_from_slice(&s.data);
        let flags = (s.read as u32) << 2 | (s.write as u32) << 1 | (s.exec as u32);
        ph.push((flags, off, s.addr, s.data.len() as u64, s.mem_size.max(s.data.len() as u64)));
        off += s.data.len() as u64;
    }
    let machine: u16 = match isa {
        Isa::X86_64 => 62,
        Isa::Aarch64 => 183,
    };
    let mut h = Vec::new();
    h.extend_from_slice(b"\x7fELF\x02\x01\x01\x00");
    h.extend_from_slice(&[0u8; 8]);
    h.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    h.extend_from_slice(&machine.to_le_bytes());
    h.extend_from_slice(&1u32.to_le_bytes());
    h.extend_from_slice(&entry.to_le_bytes());
    h.extend_from_slice(&ph_off.to_le_bytes());
    h.extend_from_slice(&0u64.to_le_bytes()); // no section headers
    h.extend_from_slice(&0u32.to_le_bytes());
    h.extend_from_slice(&64u16.to_le_bytes());
    h.extend_from_slice(&56u16.to_le_bytes());
    h.extend_from_slice(&(n as u16).to_le_bytes());
    h.extend_from_slice(&64u16.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    for (flags, o, a, fsz, msz) in ph {
        h.extend_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        h.extend_from_slice(&flags.to_le_bytes());
        h.extend_from_slice(&o.to_le_bytes());
        h.extend_from_slice(&a.to_le_bytes());
        h.extend_from_slice(&a.to_le_bytes());
        h.extend_from_slice(&fsz.to_le_bytes());
        h.extend_from_slice(&msz.to_le_bytes());
        h.extend_from_slice(&PG.to_le_bytes());
    }
    out[..h.len()].copy_from_slice(&h);
    out
}

/// The executable bytes for this binding: the ELF as given, or the raw image wrapped.
pub fn executable(binding: &Binding, image: &Image, artifact: &[u8]) -> Vec<u8> {
    if binding.target.format == Format::Elf {
        return artifact.to_vec();
    }
    let mut segs = image.segments.clone();
    if let Some(p) = &binding.process {
        if p.data_bytes > 0 {
            segs.push(Segment { addr: DATA_BASE, data: Vec::new(), mem_size: p.data_bytes, read: true, write: true, exec: false, label: "data area".into() });
        }
    }
    wrap_elf(binding.target.isa, image.entry, &segs)
}

fn sealed_memfd(bytes: &[u8]) -> Result<i32, String> {
    unsafe {
        let fd = libc::memfd_create(c"mukoz-subject".as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING);
        if fd < 0 {
            return Err("memfd_create failed".into());
        }
        let mut off = 0;
        while off < bytes.len() {
            let w = libc::write(fd, bytes[off..].as_ptr() as *const _, bytes.len() - off);
            if w <= 0 {
                libc::close(fd);
                return Err("writing the memfd failed".into());
            }
            off += w as usize;
        }
        if libc::fcntl(fd, libc::F_ADD_SEALS, libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL) != 0 {
            libc::close(fd);
            return Err("sealing the memfd failed".into());
        }
        Ok(fd)
    }
}

fn bytes_of(v: Value) -> Vec<u8> {
    match v {
        Value::Bytes(b) => b,
        Value::Bv(w, x) => x.to_le_bytes()[..(w as usize).div_ceil(8)].to_vec(),
        Value::Bool(b) => vec![b as u8],
    }
}

fn set_nonblock(fd: i32) {
    unsafe {
        let fl = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
    }
}

/// Read what is available from `fd` into `buf`, keeping at most `cap + 1` bytes.
/// Returns false at EOF.
fn drain(fd: i32, buf: &mut Vec<u8>, cap: usize) -> bool {
    let mut tmp = [0u8; 16384];
    loop {
        let r = unsafe { libc::read(fd, tmp.as_mut_ptr() as *mut _, tmp.len()) };
        if r > 0 {
            let room = (cap + 1).saturating_sub(buf.len());
            buf.extend_from_slice(&tmp[..(r as usize).min(room)]);
            continue;
        }
        return r != 0;
    }
}

pub struct ProcRun {
    pub obs: Observation,
    pub applied_isolation: Vec<&'static str>,
}

fn blank(stop: Stop) -> Observation {
    Observation { stop, regs_in: BTreeMap::new(), regs_out: BTreeMap::new(), flags_out: 0, regions_out: BTreeMap::new(), recent: Vec::new(), instructions: 0, process: None, monitors: BTreeMap::new() }
}

fn stream_cap(binding: &Binding, contract: &Contract, which: &str) -> usize {
    binding.results.iter().find(|(_, src)| src == which).and_then(|(name, _)| contract.results.get(name)).map(|v| v.max_len as usize).unwrap_or(DEFAULT_CAP)
}

/// Run one process case natively.
pub fn run(exe: &[u8], contract: &Contract, binding: &Binding, case: &Case, wall_ms: u64) -> ProcRun {
    if let Err(e) = host_can_execute(binding) {
        return ProcRun { obs: blank(Stop::SetupError { error: e }), applied_isolation: vec![] };
    }
    match run_inner(exe, contract, binding, case, wall_ms) {
        Ok(r) => r,
        Err(e) => ProcRun { obs: blank(Stop::SetupError { error: e }), applied_isolation: vec![] },
    }
}

fn run_inner(exe: &[u8], contract: &Contract, binding: &Binding, case: &Case, wall_ms: u64) -> Result<ProcRun, String> {
    let p = binding.process.as_ref().ok_or("process binding without [process]")?;
    let empty = BTreeMap::new();
    let cx = EvalCtx { vars: &case.values, region_addrs: &empty };
    // argv / stdin exactly as the emulated executor builds them.
    let mut argv: Vec<Vec<u8>> = vec![p.argv0.as_bytes().to_vec()];
    let argc_used = match &p.argc {
        Some(e) => match expr::eval(e, &cx).map_err(|m| format!("process.argc: {m}"))? {
            Value::Bv(_, n) if n as usize <= p.argv.len() => n as usize,
            Value::Bv(_, n) => return Err(format!("BINDING_MISMATCH: process.argc = {n} but only {} argv expressions are bound", p.argv.len())),
            _ => return Err("process.argc is not a bitvector".into()),
        },
        None => p.argv.len(),
    };
    for (i, e) in p.argv.iter().take(argc_used).enumerate() {
        let b = bytes_of(expr::eval(e, &cx).map_err(|m| format!("process.argv[{i}]: {m}"))?);
        if b.contains(&0) {
            return Err(format!("process.argv[{i}] contains a NUL byte"));
        }
        argv.push(b);
    }
    let stdin = match &p.stdin {
        Some(e) => bytes_of(expr::eval(e, &cx).map_err(|m| format!("process.stdin: {m}"))?),
        None => Vec::new(),
    };
    // The root directory holds only the declared files.
    let root = host::make_trial_dir("proc");
    let cleanup = |r| {
        host::remove_dir(&root);
        r
    };
    let mut caps = BTreeMap::new();
    for f in &binding.files {
        let exists = match &f.exists {
            Some(e) => matches!(expr::eval(e, &cx).map_err(|m| format!("file {} exists: {m}", f.name))?, Value::Bool(true)),
            None => true,
        };
        let rel = f.path.trim_start_matches('/');
        if rel.is_empty() || rel.split('/').any(|c| c == ".." || c.is_empty()) {
            return cleanup(Err(format!("file {}: path `{}` cannot be placed in the sandbox", f.name, f.path)));
        }
        let path = root.join(rel);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        if exists {
            let data = match &f.init {
                Some(e) => bytes_of(expr::eval(e, &cx).map_err(|m| format!("file {} init: {m}", f.name))?),
                None => Vec::new(),
            };
            std::fs::write(&path, data).map_err(|e| e.to_string())?;
        }
        let cap = f.observe_state.as_ref().and_then(|s| contract.state.get(s)).map(|v| v.max_len as usize).unwrap_or(DEFAULT_CAP);
        caps.insert(f.name.clone(), (path, cap));
    }
    let fd = sealed_memfd(exe)?;
    let mut pin = [0i32; 2];
    let mut pout = [0i32; 2];
    let mut perr = [0i32; 2];
    // Start report: the payload writes 'S' just before execveat and 'F' if it returns. The pipe is
    // close-on-exec, so "S" alone means the program started; nothing means the sandbox failed.
    let mut prep = [0i32; 2];
    unsafe {
        if libc::pipe2(pin.as_mut_ptr(), libc::O_CLOEXEC) != 0
            || libc::pipe2(pout.as_mut_ptr(), libc::O_CLOEXEC) != 0
            || libc::pipe2(perr.as_mut_ptr(), libc::O_CLOEXEC) != 0
            || libc::pipe2(prep.as_mut_ptr(), libc::O_CLOEXEC) != 0
        {
            libc::close(fd);
            return cleanup(Err("pipe failed".into()));
        }
    }
    let c_argv: Vec<CString> = argv.iter().map(|a| CString::new(a.clone()).unwrap()).collect();
    let mut argv_ptrs: Vec<*const libc::c_char> = c_argv.iter().map(|c| c.as_ptr()).collect();
    argv_ptrs.push(std::ptr::null());
    let envp: [*const libc::c_char; 1] = [std::ptr::null()];
    let cpu_s = wall_ms.div_ceil(1000) + 1;
    let sb = Sandbox::trial(Some(root.clone()), cpu_s);
    let applied = sb.applied();
    let keep = [fd, pin[0], pout[1], perr[1], prep[1]];
    let (in_r, out_w, err_w, rep_w) = (pin[0], pout[1], perr[1], prep[1]);
    let payload = move || unsafe {
        // dup2 clears close-on-exec on 0..2; the originals close at exec.
        if libc::dup2(in_r, 0) < 0 || libc::dup2(out_w, 1) < 0 || libc::dup2(err_w, 2) < 0 {
            libc::write(rep_w, b"D".as_ptr() as *const _, 1);
            return 1;
        }
        // Files are opened relative to the working directory: the sandbox root holds them.
        libc::chdir(c"/".as_ptr());
        libc::write(rep_w, b"S".as_ptr() as *const _, 1);
        libc::syscall(libc::SYS_execveat, fd, c"".as_ptr(), argv_ptrs.as_ptr(), envp.as_ptr(), libc::AT_EMPTY_PATH);
        libc::write(rep_w, b"F".as_ptr() as *const _, 1);
        1
    };
    // Parent ends: write stdin, read stdout/stderr while the child runs. The child's ends stay
    // open in the parent until the run is over (they are what the child inherits at fork).
    let (in_w, out_r, err_r) = (pin[1], pout[0], perr[0]);
    for f in [in_w, out_r, err_r] {
        set_nonblock(f);
    }
    let out_cap = stream_cap(binding, contract, "stdout");
    let err_cap = stream_cap(binding, contract, "stderr");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut in_off = 0usize;
    let mut in_open = true;
    let mut over: Option<&'static str> = None;
    let mut tick = || {
        if in_open {
            while in_off < stdin.len() {
                let w = unsafe { libc::write(in_w, stdin[in_off..].as_ptr() as *const _, stdin.len() - in_off) };
                if w <= 0 {
                    break;
                }
                in_off += w as usize;
            }
            if in_off >= stdin.len() {
                unsafe { libc::close(in_w) };
                in_open = false;
            }
        }
        drain(out_r, &mut stdout, out_cap);
        drain(err_r, &mut stderr, err_cap);
        if stdout.len() > out_cap {
            over = Some("stdout");
        } else if stderr.len() > err_cap {
            over = Some("stderr");
        }
        over.is_some()
    };
    // The payload holds raw pointers into c_argv; c_argv lives until after the run.
    let exit = host::run_tick(&sb, wall_ms, &keep, payload, &mut tick);
    drop(tick);
    unsafe {
        libc::close(pin[0]);
        libc::close(pout[1]);
        libc::close(perr[1]);
        libc::close(prep[1]);
    }
    drain(out_r, &mut stdout, out_cap);
    drain(err_r, &mut stderr, err_cap);
    let mut report = Vec::new();
    set_nonblock(prep[0]);
    drain(prep[0], &mut report, 8);
    unsafe {
        if in_open {
            libc::close(in_w);
        }
        libc::close(out_r);
        libc::close(err_r);
        libc::close(fd);
        libc::close(prep[0]);
    }
    drop(c_argv);
    if over.is_none() && stdout.len() > out_cap {
        over = Some("stdout");
    }
    if over.is_none() && stderr.len() > err_cap {
        over = Some("stderr");
    }
    let mut files = BTreeMap::new();
    for (name, (path, cap)) in &caps {
        match std::fs::read(path) {
            Ok(mut d) => {
                d.truncate(cap + 1);
                files.insert(name.clone(), (true, d));
            }
            Err(_) => {
                files.insert(name.clone(), (false, Vec::new()));
            }
        }
    }
    host::remove_dir(&root);
    let mut exit_status = None;
    let stop = match (over, exit) {
        _ if report.is_empty() => Stop::SetupError { error: format!("the sandbox could not be entered ({exit:?})") },
        _ if report == b"D" => Stop::SetupError { error: "redirecting the standard streams failed".into() },
        _ if report == b"SF" => Stop::SetupError { error: "EXECUTION_BLOCKED_PLATFORM_POLICY: execveat refused the image (noexec, policy, or an image the kernel cannot load)".into() },
        (Some(stream), _) => Stop::OutputLimit { pc_offset: "unknown (native)".into(), stream: stream.into(), limit: if stream == "stdout" { out_cap } else { err_cap } },
        (None, Exit::Code(c)) => {
            exit_status = Some(c as u64);
            Stop::Exited { status: c as u64 }
        }
        (None, Exit::TimedOut) | (None, Exit::Killed) => Stop::Timeout { instructions: 0 },
        (None, Exit::Signal(s)) if s == libc::SIGXCPU || s == libc::SIGKILL => Stop::Timeout { instructions: 0 },
        (None, Exit::Signal(s)) if s == libc::SIGSEGV || s == libc::SIGBUS => Stop::MemoryViolation {
            pc_offset: "unknown (native)".into(),
            access: "unknown".into(),
            address: "unknown".into(),
            size: 0,
            detail: format!("native-process: killed by signal {s}; the faulting access is not observed"),
        },
        (None, Exit::Signal(s)) if s == libc::SIGILL => Stop::InvalidInstruction { pc_offset: "unknown (native)".into() },
        (None, Exit::Signal(s)) => Stop::ForbiddenEffect { pc_offset: "unknown (native)".into(), effect: format!("killed by signal {s}") },
    };
    let process = ProcOut { exit_status, stdout, stderr, files, syscalls: Vec::new(), syscall_count: 0 };
    Ok(ProcRun { obs: Observation { process: Some(process), ..blank(stop) }, applied_isolation: applied })
}
