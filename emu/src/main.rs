// SPDX-License-Identifier: GPL-2.0-or-later
//! mukoz-emu: a machine emulator process (Unicorn) driven over the `mukoz-emu/1` line protocol
//! on stdin / stdout. It knows nothing about contracts: it maps memory, sets registers, runs,
//! stops on accesses outside the ranges it is given, and asks its client what to do at every
//! system call, interrupt and breakpoint. The protocol is documented in PROTOCOL.md.
//!
//! One fresh engine instance per `run`: re-using an instance after rewriting code was observed
//! to execute stale translated blocks.

use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, Write};
use std::rc::Rc;
use unicorn_engine::unicorn_const::{Arch, HookType, MemType, Mode, Permission, uc_error};
use unicorn_engine::{InsnSysX86, RegisterARM64, RegisterX86, Unicorn};

const PROTOCOL: &str = "mukoz-emu/1";
const ENGINE: &str = "unicorn-engine 2.1.1 (bundled C, crate pin)";
const RING: usize = 64;

#[derive(Clone, Copy, PartialEq)]
enum Isa {
    X86_64,
    Aarch64,
}

struct Io {
    input: std::io::StdinLock<'static>,
    out: std::io::StdoutLock<'static>,
}

impl Io {
    fn send(&mut self, v: &Value) {
        let mut s = v.to_string();
        s.push('\n');
        if self.out.write_all(s.as_bytes()).and_then(|_| self.out.flush()).is_err() {
            std::process::exit(0);
        }
    }
    fn recv(&mut self) -> Value {
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) | Err(_) => std::process::exit(0),
            Ok(_) => serde_json::from_str(&line).unwrap_or_else(|e| json!({ "op": "invalid", "error": e.to_string() })),
        }
    }
}

fn reg_id(isa: Isa, name: &str) -> Option<i32> {
    Some(match isa {
        Isa::X86_64 => match name {
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
            "rip" => RegisterX86::RIP,
            "rflags" => RegisterX86::RFLAGS,
            _ => return None,
        }
        .into(),
        Isa::Aarch64 => match name {
            "sp" => RegisterARM64::SP.into(),
            "pc" => RegisterARM64::PC.into(),
            "nzcv" => RegisterARM64::NZCV.into(),
            "x29" => RegisterARM64::X29.into(),
            "x30" => RegisterARM64::X30.into(),
            _ => {
                let n: i32 = name.strip_prefix('x')?.parse().ok()?;
                if !(0..=28).contains(&n) {
                    return None;
                }
                // X0..X28 are contiguous in Unicorn's enum.
                let x0: i32 = RegisterARM64::X0.into();
                x0 + n
            }
        },
    })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn u(v: &Value, k: &str) -> Result<u64, String> {
    v[k].as_u64().ok_or_else(|| format!("`{k}` must be an unsigned integer"))
}

/// Per-run state shared by the hooks.
struct State {
    io: Io,
    isa: Isa,
    stop: Option<Value>,
    count: u64,
    ring: VecDeque<(u64, u32)>,
    breaks: HashSet<u64>,
}

impl State {
    fn last_pc(&self) -> Option<u64> {
        self.ring.back().map(|(a, _)| *a)
    }
    fn set_stop(&mut self, v: Value) {
        if self.stop.is_none() {
            self.stop = Some(v);
        }
    }
}

/// Answer machine operations until the client says `continue` (true) or `stop` (false).
/// Outside an event (after the run) `done` ends the conversation and returns true.
fn serve(uc: &mut Unicorn<()>, st: &mut State, in_event: bool) -> bool {
    loop {
        let m = st.io.recv();
        let isa = st.isa;
        let reply = match m["op"].as_str().unwrap_or("") {
            "continue" if in_event => return true,
            "stop" if in_event => return false,
            "done" if !in_event => {
                st.io.send(&json!({ "ok": true }));
                return true;
            }
            "reg_read" => {
                let mut out = serde_json::Map::new();
                let mut err = None;
                for n in m["names"].as_array().cloned().unwrap_or_default() {
                    let n = n.as_str().unwrap_or("").to_string();
                    match reg_id(isa, &n) {
                        Some(id) => {
                            out.insert(n, json!(uc.reg_read(id).unwrap_or(0)));
                        }
                        None => err = Some(format!("unknown register `{n}`")),
                    }
                }
                match err {
                    Some(e) => json!({ "error": e }),
                    None => json!({ "regs": out }),
                }
            }
            "reg_write" => {
                let mut err = None;
                for (n, v) in m["regs"].as_object().cloned().unwrap_or_default() {
                    match (reg_id(isa, &n), v.as_u64()) {
                        (Some(id), Some(v)) => {
                            let _ = uc.reg_write(id, v);
                        }
                        _ => err = Some(format!("bad register write `{n}`")),
                    }
                }
                match err {
                    Some(e) => json!({ "error": e }),
                    None => json!({ "ok": true }),
                }
            }
            "mem_read" => match (u(&m, "addr"), u(&m, "len")) {
                (Ok(a), Ok(n)) if n <= 64 << 20 => match uc.mem_read_as_vec(a, n as usize) {
                    Ok(b) => json!({ "hex": hex(&b) }),
                    Err(e) => json!({ "error": format!("{e:?}") }),
                },
                _ => json!({ "error": "mem_read needs addr and len (at most 64 MiB)" }),
            },
            "mem_write" => match (u(&m, "addr"), m["hex"].as_str().and_then(unhex)) {
                (Ok(a), Some(b)) => match uc.mem_write(a, &b) {
                    Ok(()) => json!({ "ok": true }),
                    Err(e) => json!({ "error": format!("{e:?}") }),
                },
                _ => json!({ "error": "mem_write needs addr and hex" }),
            },
            "break_add" | "break_remove" => match u(&m, "addr") {
                Ok(a) => {
                    if m["op"] == "break_add" {
                        st.breaks.insert(a);
                    } else {
                        st.breaks.remove(&a);
                    }
                    json!({ "ok": true })
                }
                Err(e) => json!({ "error": e }),
            },
            other => json!({ "error": format!("unexpected op `{other}` here") }),
        };
        st.io.send(&reply);
    }
}

/// Ask the client about an event raised inside a hook; stop the engine if it says so.
fn event(uc: &mut Unicorn<()>, st: &mut State, v: Value) {
    st.io.send(&v);
    if !serve(uc, st, true) {
        st.set_stop(json!({ "kind": "client" }));
        let _ = uc.emu_stop();
    }
}

fn ranges(v: &Value, k: &str) -> Vec<Vec<u64>> {
    v[k].as_array().cloned().unwrap_or_default().iter().map(|r| r.as_array().cloned().unwrap_or_default().iter().map(|x| x.as_u64().unwrap_or(0)).collect()).collect()
}

/// Is [addr, addr+len) inside one range that allows this kind of access?
fn allowed_ok(allowed: &[(u64, u64, bool, bool)], addr: u64, len: u64, write: bool) -> bool {
    if len == 0 {
        return true;
    }
    let Some(end) = addr.checked_add(len) else { return false };
    allowed.iter().any(|(lo, hi, r, w)| addr >= *lo && end <= *hi && if write { *w } else { *r })
}

fn run(io: Io, spec: &Value) -> Io {
    let isa = match spec["isa"].as_str() {
        Some("x86_64") => Isa::X86_64,
        Some("aarch64") => Isa::Aarch64,
        _ => {
            let mut io = io;
            io.send(&json!({ "event": "setup_error", "error": "isa must be x86_64 or aarch64" }));
            return io;
        }
    };
    let st = Rc::new(RefCell::new(State { io, isa, stop: None, count: 0, ring: VecDeque::with_capacity(RING), breaks: HashSet::new() }));
    let r = run_inner(isa, spec, &st);
    let st = match Rc::try_unwrap(st) {
        Ok(c) => c.into_inner(),
        Err(_) => unreachable!("hooks are dropped with the engine"),
    };
    let mut st = st;
    if let Err(e) = r {
        st.io.send(&json!({ "event": "setup_error", "error": e }));
    }
    st.io
}

fn run_inner(isa: Isa, spec: &Value, st: &Rc<RefCell<State>>) -> Result<(), String> {
    let ue = |e: uc_error| format!("engine: {e:?}");
    let mut uc = match isa {
        Isa::X86_64 => Unicorn::new(Arch::X86, Mode::MODE_64),
        Isa::Aarch64 => Unicorn::new(Arch::ARM64, Mode::ARM),
    }
    .map_err(ue)?;
    for m in spec["map"].as_array().cloned().unwrap_or_default() {
        let perm = match m["perm"].as_str() {
            Some("rwx") => Permission::ALL,
            Some("rw") => Permission::READ | Permission::WRITE,
            _ => return Err("map.perm must be rwx or rw".into()),
        };
        uc.mem_map(u(&m, "addr")?, u(&m, "size")? as usize, perm).map_err(ue)?;
    }
    for w in spec["write"].as_array().cloned().unwrap_or_default() {
        let b = w["hex"].as_str().and_then(unhex).ok_or("write.hex is not hex")?;
        uc.mem_write(u(&w, "addr")?, &b).map_err(ue)?;
    }
    // Registers in the order given (an array of [name, value]).
    for r in spec["regs"].as_array().cloned().unwrap_or_default() {
        let n = r[0].as_str().unwrap_or("");
        let id = reg_id(isa, n).ok_or_else(|| format!("unknown register `{n}`"))?;
        uc.reg_write(id, r[1].as_u64().ok_or("register value must be an unsigned integer")?).map_err(ue)?;
    }
    let allowed: Rc<Vec<(u64, u64, bool, bool)>> = Rc::new(ranges(spec, "allowed").iter().map(|r| (r[0], r[1], r[2] != 0, r[3] != 0)).collect());
    let code: Vec<(u64, u64)> = ranges(spec, "code").iter().map(|r| (r[0], r[1])).collect();
    let frozen: Vec<(String, i32, u64)> = spec["frozen"]
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|(n, v)| Ok((n.clone(), reg_id(isa, &n).ok_or_else(|| format!("unknown register `{n}`"))?, v.as_u64().unwrap_or(0))))
        .collect::<Result<_, String>>()?;
    st.borrow_mut().breaks = spec["breaks"].as_array().cloned().unwrap_or_default().iter().filter_map(|a| a.as_u64()).collect();
    let sp_id: i32 = match isa {
        Isa::X86_64 => RegisterX86::RSP.into(),
        Isa::Aarch64 => RegisterARM64::SP.into(),
    };

    // Every instruction: undecodable marker, code range, frozen registers, ring, breakpoints.
    {
        let st = st.clone();
        uc.add_code_hook(0, u64::MAX, move |uc, addr, size| {
            let mut s = st.borrow_mut();
            if s.stop.is_some() {
                return;
            }
            // Unicorn reports an undecodable instruction with this size marker.
            if size == 0xf1f1_f1f1 {
                s.set_stop(json!({ "kind": "undecodable", "pc": addr }));
                drop(s);
                let _ = uc.emu_stop();
                return;
            }
            if !code.iter().any(|(lo, hi)| addr >= *lo && addr + size as u64 <= *hi) {
                let from = s.last_pc();
                s.set_stop(json!({ "kind": "left_code", "from": from, "target": addr }));
                drop(s);
                let _ = uc.emu_stop();
                return;
            }
            for (name, id, v0) in &frozen {
                if uc.reg_read(*id).unwrap_or(*v0) != *v0 {
                    let pc = s.last_pc();
                    s.set_stop(json!({ "kind": "frozen", "name": name, "pc": pc }));
                    drop(s);
                    let _ = uc.emu_stop();
                    return;
                }
            }
            s.count += 1;
            if s.ring.len() == RING {
                s.ring.pop_front();
            }
            s.ring.push_back((addr, size));
            if s.breaks.contains(&addr) {
                let sp = uc.reg_read(sp_id).unwrap_or(0);
                event(uc, &mut *s, json!({ "event": "break", "pc": addr, "sp": sp }));
            }
        })
        .map_err(ue)?;
    }
    // Data accesses outside the allowed ranges.
    {
        let st = st.clone();
        let allowed = allowed.clone();
        uc.add_mem_hook(HookType::MEM_READ | HookType::MEM_WRITE, 0, u64::MAX, move |uc, t, addr, size, _v| {
            let write = matches!(t, MemType::WRITE);
            if !allowed_ok(&allowed, addr, size as u64, write) {
                let pc = uc.pc_read().unwrap_or(0);
                st.borrow_mut().set_stop(json!({ "kind": "access", "write": write, "addr": addr, "size": size, "pc": pc }));
                let _ = uc.emu_stop();
            }
            true
        })
        .map_err(ue)?;
    }
    // Unmapped or protected accesses (the engine faults; record where).
    {
        let st = st.clone();
        uc.add_mem_hook(HookType::MEM_INVALID, 0, u64::MAX, move |uc, t, addr, size, _v| {
            let pc = uc.pc_read().unwrap_or(0);
            let mut s = st.borrow_mut();
            let v = match t {
                MemType::FETCH_UNMAPPED | MemType::FETCH_PROT => json!({ "kind": "left_code", "from": s.last_pc(), "target": addr }),
                _ => json!({ "kind": "fault", "write": matches!(t, MemType::WRITE_UNMAPPED | MemType::WRITE_PROT), "addr": addr, "size": size, "pc": pc }),
            };
            s.set_stop(v);
            false
        })
        .map_err(ue)?;
    }
    // Interrupts and exceptions (A64 `svc`, `brk`, undefined instructions, x86 `int`).
    {
        let st = st.clone();
        uc.add_intr_hook(move |uc, intno| {
            let mut s = st.borrow_mut();
            if s.stop.is_some() {
                let _ = uc.emu_stop();
                return;
            }
            let pc = s.last_pc().unwrap_or_else(|| uc.pc_read().unwrap_or(0));
            event(uc, &mut *s, json!({ "event": "interrupt", "intno": intno, "pc": pc }));
        })
        .map_err(ue)?;
    }
    if isa == Isa::X86_64 {
        for (kind, name) in [(InsnSysX86::SYSCALL, "syscall"), (InsnSysX86::SYSENTER, "sysenter")] {
            let st = st.clone();
            uc.add_insn_sys_hook(kind, 0, u64::MAX, move |uc| {
                let mut s = st.borrow_mut();
                if s.stop.is_some() {
                    return;
                }
                let pc = s.last_pc().unwrap_or(0);
                event(uc, &mut *s, json!({ "event": "syscall", "insn": name, "pc": pc }));
            })
            .map_err(ue)?;
        }
    }

    let started = std::time::Instant::now();
    let res = uc.emu_start(u(spec, "entry")?, u(spec, "until")?, u(spec, "timeout_ms")? * 1000, u(spec, "insn_limit")? as usize);
    let elapsed = started.elapsed().as_millis() as u64;
    let pc = uc.pc_read().unwrap_or(0);
    let sp = uc.reg_read(sp_id).unwrap_or(0);
    let mut s = st.borrow_mut();
    let end = json!({
        "event": "end",
        "stop": s.stop,
        "error": res.err().map(|e| format!("{e:?}")),
        "pc": pc,
        "sp": sp,
        "count": s.count,
        "elapsed_ms": elapsed,
        "ring": s.ring.iter().map(|(a, n)| json!([a, n])).collect::<Vec<_>>(),
    });
    s.io.send(&end);
    serve(&mut uc, &mut s, false);
    Ok(())
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("mukoz-emu {} ({PROTOCOL}; {ENGINE})", env!("CARGO_PKG_VERSION"));
        return;
    }
    let mut io = Io { input: std::io::stdin().lock(), out: std::io::stdout().lock() };
    loop {
        let m = io.recv();
        match m["op"].as_str().unwrap_or("") {
            "hello" => io.send(&json!({ "protocol": PROTOCOL, "program": env!("CARGO_PKG_NAME"), "engine": ENGINE, "version": env!("CARGO_PKG_VERSION") })),
            "run" => io = run(io, &m),
            "quit" => return,
            other => io.send(&json!({ "error": format!("unknown op `{other}`") })),
        }
    }
}
