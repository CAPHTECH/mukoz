// SPDX-License-Identifier: MIT OR Apache-2.0
//! mukoz-emu-icicle: a machine emulator process driven over the `mukoz-emu/1` line protocol
//! (../emu/PROTOCOL.md), built on icicle-emu, a SLEIGH / p-code interpreter that shares no code
//! with Unicorn or QEMU. It knows nothing about contracts: it maps memory, sets registers, runs,
//! stops on accesses outside the ranges it is given, and asks its client what to do at every
//! system call, interrupt and breakpoint.
//!
//! How the protocol maps onto icicle:
//! - Every lifted block is instrumented: a hook after each instruction marker (code ranges, frozen
//!   registers, budget, time, ring, breakpoints) and a hook before each RAM load and store (the
//!   allowed ranges, checked before the access happens).
//! - System calls, interrupts and faults end `Vm::run` with an exception; the event is served
//!   outside the engine and execution resumes after the instruction.
//! - One engine per ISA, reset before every run (memory, registers and translated code).

use icicle_vm::cpu::{Config, Cpu, Exception, ExceptionCode, StoreRef, mem::{Mapping, perm}};
use icicle_vm::{BlockTable, CodeInjector, Vm, VmExit};
use pcode::{Op, Value as PValue, VarNode};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, Write};
use std::rc::Rc;
use std::time::{Duration, Instant};

mod sleigh {
    include!(concat!(env!("OUT_DIR"), "/sleigh_files.rs"));
}

const PROTOCOL: &str = "mukoz-emu/1";
const ENGINE: &str = "icicle-emu git 3292602fd485 (interpreter; SLEIGH specs from icicle-emu/ghidra 50230050fa58)";
const RING: usize = 64;
/// Exception values marking a stop requested by this program's own hooks, and a jump the client
/// made during a break event (resumed at the new pc).
const HALT: u64 = 0x6d75_6b6f_7a00;
const REDIRECT: u64 = 0x6d75_6b6f_7a01;

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

/// The protocol's register names on top of SLEIGH's. SLEIGH keeps each status flag in its own
/// register, so `rflags` and `nzcv` are assembled from (and split into) those.
struct Regs {
    isa: Isa,
    named: HashMap<String, VarNode>,
    /// (bit, flag register); `fixed` bits always read as set (x86 bit 1).
    flags: Vec<(u32, VarNode)>,
    fixed: u64,
    sp: VarNode,
}

impl Regs {
    fn new(vm: &Vm, isa: Isa) -> Result<Regs, String> {
        let sleigh = &vm.cpu.arch.sleigh;
        let var = |n: &str| -> Result<VarNode, String> { sleigh.get_reg(n).and_then(|r| r.get_var()).ok_or_else(|| format!("SLEIGH register `{n}` missing")) };
        let mut named = HashMap::new();
        let (flag_names, fixed, sp): (&[(u32, &str)], u64, &str) = match isa {
            Isa::X86_64 => {
                for n in ["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"] {
                    named.insert(n.to_string(), var(&n.to_uppercase())?);
                }
                (&[(0, "CF"), (2, "PF"), (4, "AF"), (6, "ZF"), (7, "SF"), (8, "TF"), (9, "IF"), (10, "DF"), (11, "OF"), (14, "NT"), (18, "AC"), (21, "ID")], 0x2, "RSP")
            }
            Isa::Aarch64 => {
                for i in 0..=30 {
                    named.insert(format!("x{i}"), var(&format!("x{i}"))?);
                }
                named.insert("sp".into(), var("sp")?);
                (&[(31, "NG"), (30, "ZR"), (29, "CY"), (28, "OV")], 0, "sp")
            }
        };
        let flags = flag_names.iter().map(|(b, n)| Ok((*b, var(n)?))).collect::<Result<_, String>>()?;
        Ok(Regs { isa, named, flags, fixed, sp: var(sp)? })
    }

    fn is_pc(&self, name: &str) -> bool {
        name == match self.isa {
            Isa::X86_64 => "rip",
            Isa::Aarch64 => "pc",
        }
    }

    fn is_flags(&self, name: &str) -> bool {
        name == match self.isa {
            Isa::X86_64 => "rflags",
            Isa::Aarch64 => "nzcv",
        }
    }

    fn known(&self, name: &str) -> bool {
        self.is_pc(name) || self.is_flags(name) || self.named.contains_key(name)
    }

    fn read(&self, cpu: &mut Cpu, name: &str) -> Option<u64> {
        if self.is_pc(name) {
            return Some(cpu.read_pc());
        }
        if self.is_flags(name) {
            return Some(self.flags.iter().fold(self.fixed, |a, (b, v)| a | ((cpu.read_reg(*v) & 1) << b)));
        }
        self.named.get(name).map(|v| cpu.read_reg(*v))
    }

    fn write(&self, cpu: &mut Cpu, name: &str, val: u64) -> bool {
        if self.is_pc(name) {
            cpu.write_pc(val);
            return true;
        }
        if self.is_flags(name) {
            for (b, v) in &self.flags {
                cpu.write_reg(*v, (val >> b) & 1);
            }
            return true;
        }
        match self.named.get(name) {
            Some(v) => {
                cpu.write_reg(*v, val);
                true
            }
            None => false,
        }
    }
}

/// Per-run state shared by the hooks and the run loop.
struct State {
    io: Io,
    stop: Option<Value>,
    count: u64,
    ring: VecDeque<(u64, u32)>,
    breaks: HashSet<u64>,
    /// Instruction lengths, from the instruction markers of lifted code.
    lens: HashMap<u64, u32>,
    allowed: Vec<(u64, u64, bool, bool)>,
    code: Vec<(u64, u64)>,
    frozen: Vec<(String, u64)>,
    until: u64,
    insn_limit: u64,
    deadline: Instant,
    ticks: u64,
    /// The last data access seen: (addr, size, write, pc).
    last_access: Option<(u64, u64, bool, u64)>,
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
    fn timed_out(&mut self) -> bool {
        self.ticks += 1;
        self.ticks % 256 == 0 && Instant::now() >= self.deadline
    }
}

fn halt(cpu: &mut Cpu) {
    cpu.exception = Exception::new(ExceptionCode::Environment, HALT);
}

/// Answer machine operations until the client says `continue` (true) or `stop` (false).
/// Outside an event (after the run) `done` ends the conversation and returns true.
fn serve(cpu: &mut Cpu, st: &mut State, regs: &Regs, in_event: bool) -> bool {
    loop {
        let m = st.io.recv();
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
                    match regs.read(cpu, &n) {
                        Some(v) => {
                            out.insert(n, json!(v));
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
                    if !v.as_u64().is_some_and(|v| regs.write(cpu, &n, v)) {
                        err = Some(format!("bad register write `{n}`"));
                    }
                }
                match err {
                    Some(e) => json!({ "error": e }),
                    None => json!({ "ok": true }),
                }
            }
            "mem_read" => match (u(&m, "addr"), u(&m, "len")) {
                (Ok(a), Ok(n)) if n <= 64 << 20 => {
                    let mut b = vec![0u8; n as usize];
                    match cpu.mem.read_bytes(a, &mut b, perm::NONE) {
                        Ok(()) => json!({ "hex": hex(&b) }),
                        Err(e) => json!({ "error": format!("{e:?}") }),
                    }
                }
                _ => json!({ "error": "mem_read needs addr and len (at most 64 MiB)" }),
            },
            "mem_write" => match (u(&m, "addr"), m["hex"].as_str().and_then(unhex)) {
                (Ok(a), Some(b)) => match cpu.mem.write_bytes(a, &b, perm::NONE) {
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

/// Ask the client about an event; record a client stop if it says so. Returns true to continue.
fn event(cpu: &mut Cpu, st: &mut State, regs: &Regs, v: Value) -> bool {
    st.io.send(&v);
    let go = serve(cpu, st, regs, true);
    if !go {
        st.set_stop(json!({ "kind": "client" }));
    }
    go
}

/// Is [addr, addr+len) inside one range that allows this kind of access?
fn allowed_ok(allowed: &[(u64, u64, bool, bool)], addr: u64, len: u64, write: bool) -> bool {
    if len == 0 {
        return true;
    }
    let Some(end) = addr.checked_add(len) else { return false };
    allowed.iter().any(|(lo, hi, r, w)| addr >= *lo && end <= *hi && if write { *w } else { *r })
}

fn ranges(v: &Value, k: &str) -> Vec<Vec<u64>> {
    v[k].as_array().cloned().unwrap_or_default().iter().map(|r| r.as_array().cloned().unwrap_or_default().iter().map(|x| x.as_u64().unwrap_or(0)).collect()).collect()
}

/// Inserts the instruction hook after every instruction marker and the access hook (with the
/// address and size written to a trace store first) before every RAM load and store.
struct Instrument {
    insn_hook: pcode::HookId,
    access_hook: pcode::HookId,
    store_id: u16,
    st: Rc<RefCell<State>>,
    tmp: pcode::Block,
}

impl Instrument {
    fn check(&mut self, addr: PValue, size: u8, write: bool) {
        let a = match addr {
            PValue::Const(v, _) => PValue::from(v),
            PValue::Var(v) if v.size == 8 => addr,
            PValue::Var(_) => {
                let t = self.tmp.alloc_tmp(8);
                self.tmp.push((t, Op::ZeroExtend, addr));
                t.into()
            }
        };
        self.tmp.push((Op::Store(self.store_id), (0_u64, a)));
        self.tmp.push((Op::Store(self.store_id), (8_u64, size as u64 | (write as u64) << 32)));
        self.tmp.push(Op::Hook(self.access_hook));
    }
}

impl CodeInjector for Instrument {
    fn inject(&mut self, _cpu: &mut Cpu, group: &icicle_vm::cpu::BlockGroup, code: &mut BlockTable) {
        for id in group.range() {
            let block = &mut code.blocks[id];
            self.tmp.clear();
            self.tmp.next_tmp = block.pcode.next_tmp;
            let stmts: Vec<pcode::Instruction> = block.pcode.instructions.drain(..).collect();
            for stmt in stmts {
                match stmt.op {
                    Op::InstructionMarker => {
                        self.tmp.push(stmt);
                        self.st.borrow_mut().lens.insert(stmt.inputs.first().as_u64(), stmt.inputs.second().as_u64() as u32);
                        self.tmp.push(Op::Hook(self.insn_hook));
                    }
                    Op::Load(pcode::RAM_SPACE) => {
                        self.check(stmt.inputs.first(), stmt.output.size, false);
                        self.tmp.push(stmt);
                    }
                    Op::Store(pcode::RAM_SPACE) => {
                        self.check(stmt.inputs.first(), stmt.inputs.second().size(), true);
                        self.tmp.push(stmt);
                    }
                    _ => self.tmp.push(stmt),
                }
            }
            std::mem::swap(&mut self.tmp.instructions, &mut block.pcode.instructions);
            block.pcode.next_tmp = self.tmp.next_tmp;
            code.modified.insert(id);
        }
    }
}

/// One engine for one ISA, reused (after a reset) for every run.
struct Machine {
    vm: Vm,
    regs: Rc<Regs>,
}

fn build(isa: Isa, processors: &std::path::Path, st: &Rc<RefCell<State>>) -> Result<Machine, String> {
    let triple = match isa {
        Isa::X86_64 => "x86_64-none",
        Isa::Aarch64 => "aarch64-none",
    };
    let cfg = Config {
        triple: triple.parse().map_err(|e| format!("{e:?}"))?,
        enable_jit: false,
        enable_jit_mem: false,
        enable_shadow_stack: false,
        enable_recompilation: false,
        track_uninitialized: false,
        optimize_instructions: false,
        optimize_block: false,
    };
    let mut vm = icicle_vm::build_with_path(&cfg, processors).map_err(|e| format!("icicle build {triple}: {e:?}"))?;
    vm.icount_limit = u64::MAX;
    let regs = Rc::new(Regs::new(&vm, isa)?);
    let store: StoreRef = vm.cpu.trace.register_store(vec![0u64; 2]);

    // Before every instruction: until, code ranges, frozen registers, budget, time, ring, breaks.
    let insn_hook = {
        let st = st.clone();
        let regs = regs.clone();
        vm.cpu.add_hook(move |cpu: &mut Cpu, addr: u64| {
            let mut s = st.borrow_mut();
            if s.stop.is_some() || addr == s.until {
                return halt(cpu);
            }
            let len = s.lens.get(&addr).copied().unwrap_or(1);
            if !s.code.iter().any(|(lo, hi)| addr >= *lo && addr + len as u64 <= *hi) {
                let from = s.last_pc();
                s.set_stop(json!({ "kind": "left_code", "from": from, "target": addr }));
                return halt(cpu);
            }
            for i in 0..s.frozen.len() {
                let (name, v0) = (s.frozen[i].0.clone(), s.frozen[i].1);
                if regs.read(cpu, &name).unwrap_or(v0) != v0 {
                    let pc = s.last_pc();
                    s.set_stop(json!({ "kind": "frozen", "name": name, "pc": pc }));
                    return halt(cpu);
                }
            }
            if s.count >= s.insn_limit || s.timed_out() {
                return halt(cpu);
            }
            s.count += 1;
            if s.ring.len() == RING {
                s.ring.pop_front();
            }
            s.ring.push_back((addr, len));
            if s.breaks.contains(&addr) {
                let sp = cpu.read_reg(regs.sp);
                if !event(cpu, &mut s, &regs, json!({ "event": "break", "pc": addr, "sp": sp })) {
                    halt(cpu);
                } else if cpu.read_pc() != addr {
                    cpu.exception = Exception::new(ExceptionCode::Environment, REDIRECT);
                }
            }
        })
    };
    // Before every data access: the allowed ranges.
    let access_hook = {
        let st = st.clone();
        vm.cpu.add_hook(move |cpu: &mut Cpu, pc: u64| {
            let d = cpu.trace[store].data();
            let addr = u64::from_le_bytes(d[0..8].try_into().unwrap());
            let info = u64::from_le_bytes(d[8..16].try_into().unwrap());
            let (size, write) = (info & 0xffff_ffff, info >> 32 & 1 == 1);
            let mut s = st.borrow_mut();
            s.last_access = Some((addr, size, write, pc));
            if !allowed_ok(&s.allowed, addr, size, write) {
                s.set_stop(json!({ "kind": "access", "write": write, "addr": addr, "size": size, "pc": pc }));
                return halt(cpu);
            }
            // A single instruction can loop over memory (x86 `rep`): keep the time limit.
            if s.timed_out() {
                halt(cpu);
            }
        })
    };
    vm.add_injector(Instrument { insn_hook, access_hook, store_id: store.get_store_id(), st: st.clone(), tmp: pcode::Block::new() });
    Ok(Machine { vm, regs })
}

/// Writes the embedded SLEIGH specifications to a fresh private directory, builds both engines
/// and removes the directory again.
fn build_all(st: &Rc<RefCell<State>>) -> Result<(Machine, Machine), String> {
    let base = std::env::temp_dir();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = base.join(format!("mukoz-emu-icicle-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let r = (|| {
        for (name, data) in sleigh::FILES {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&p, data).map_err(|e| e.to_string())?;
        }
        Ok((build(Isa::X86_64, &dir, st)?, build(Isa::Aarch64, &dir, st)?))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    r
}

/// Instruction bytes at `pc`, as far as they can be read (at most 16).
fn code_at(cpu: &mut Cpu, pc: u64) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..16 {
        let mut b = [0u8];
        if cpu.mem.read_bytes(pc + i, &mut b, perm::NONE).is_err() {
            break;
        }
        out.push(b[0]);
    }
    out
}

/// What raised icicle's `Syscall` exception on x86-64: `Ok(name)` for a system call
/// instruction, `Err(intno)` for an interrupt.
fn x86_trap(bytes: &[u8], fallback: u64) -> Result<&'static str, u64> {
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 | 0x66 | 0x67 | 0xf0 | 0xf2 | 0xf3 | 0x40..=0x4f) {
        i += 1;
    }
    match bytes.get(i..) {
        Some([0x0f, 0x05, ..]) => Ok("syscall"),
        Some([0x0f, 0x34, ..]) => Ok("sysenter"),
        Some([0xcd, n, ..]) => Err(*n as u64),
        Some([0xcc, ..]) => Err(3),
        Some([0xf1, ..]) => Err(1),
        Some([0xce, ..]) => Err(4),
        _ => Err(fallback),
    }
}

/// The exception number Unicorn (QEMU) reports for an A64 trapping instruction word.
fn a64_intno(word: Option<u32>) -> u64 {
    match word {
        Some(w) if w & 0xffe0_001f == 0xd400_0001 => 2, // svc
        Some(w) if w & 0xffe0_001f == 0xd400_0002 => 3, // hvc
        Some(w) if w & 0xffe0_001f == 0xd400_0003 => 4, // smc
        Some(w) if w & 0xffe0_001f == 0xd420_0000 => 7, // brk
        _ => 1,                                          // undefined
    }
}

/// A64 SDIV `w` with the architecture's result: MIN / -1 = MIN, x / 0 = 0.
fn a64_sdiv(cpu: &mut Cpu, regs: &Regs, w: u32) {
    let (rd, rn, rm) = (w & 31, (w >> 5) & 31, (w >> 16) & 31);
    let r = |cpu: &mut Cpu, i: u32| if i == 31 { 0 } else { regs.read(cpu, &format!("x{i}")).unwrap_or(0) };
    let (n, m) = (r(cpu, rn), r(cpu, rm));
    let v = if w >> 31 == 1 {
        let (n, m) = (n as i64, m as i64);
        if m == 0 { 0 } else { n.wrapping_div(m) as u64 }
    } else {
        let (n, m) = (n as u32 as i32, m as u32 as i32);
        if m == 0 { 0 } else { n.wrapping_div(m) as u32 as u64 }
    };
    if rd != 31 {
        regs.write(cpu, &format!("x{rd}"), v);
    }
}

fn run(m: &mut Machine, st: &Rc<RefCell<State>>, isa: Isa, spec: &Value) -> Result<(), String> {
    let vm = &mut m.vm;
    let regs = m.regs.clone();
    vm.reset();
    for b in vm.breakpoints() {
        vm.remove_breakpoint(b);
    }
    {
        let mut s = st.borrow_mut();
        s.stop = None;
        s.count = 0;
        s.ring.clear();
        s.lens.clear();
        s.last_access = None;
        s.ticks = 0;
        s.allowed = ranges(spec, "allowed").iter().map(|r| (r[0], r[1], r[2] != 0, r[3] != 0)).collect();
        s.code = ranges(spec, "code").iter().map(|r| (r[0], r[1])).collect();
        s.breaks = spec["breaks"].as_array().cloned().unwrap_or_default().iter().filter_map(|a| a.as_u64()).collect();
        s.until = u(spec, "until")?;
        s.insn_limit = u(spec, "insn_limit")?;
        s.frozen = Vec::new();
        for (n, v) in spec["frozen"].as_object().cloned().unwrap_or_default() {
            if !regs.known(&n) {
                return Err(format!("unknown register `{n}`"));
            }
            s.frozen.push((n, v.as_u64().unwrap_or(0)));
        }
    }
    for mm in spec["map"].as_array().cloned().unwrap_or_default() {
        let p = match mm["perm"].as_str() {
            Some("rwx") => perm::READ | perm::WRITE | perm::EXEC,
            Some("rw") => perm::READ | perm::WRITE,
            _ => return Err("map.perm must be rwx or rw".into()),
        };
        let (a, n) = (u(&mm, "addr")?, u(&mm, "size")?);
        if !vm.cpu.mem.map_memory_len(a, n, Mapping { perm: p | perm::INIT, value: 0 }) {
            return Err(format!("engine: cannot map [0x{a:x}, +0x{n:x})"));
        }
    }
    for w in spec["write"].as_array().cloned().unwrap_or_default() {
        let b = w["hex"].as_str().and_then(unhex).ok_or("write.hex is not hex")?;
        let a = u(&w, "addr")?;
        vm.cpu.mem.write_bytes(a, &b, perm::NONE).map_err(|e| format!("engine: write at 0x{a:x}: {e:?}"))?;
    }
    for r in spec["regs"].as_array().cloned().unwrap_or_default() {
        let n = r[0].as_str().unwrap_or("");
        let v = r[1].as_u64().ok_or("register value must be an unsigned integer")?;
        if !regs.write(&mut vm.cpu, n, v) {
            return Err(format!("unknown register `{n}`"));
        }
    }
    vm.cpu.write_pc(u(spec, "entry")?);
    vm.cpu.block_id = u64::MAX;
    let until = u(spec, "until")?;
    vm.add_breakpoint(until);
    let started = Instant::now();
    st.borrow_mut().deadline = started + Duration::from_millis(u(spec, "timeout_ms")?);

    let mut error: Option<String> = None;
    loop {
        let exit = vm.run();
        let pc = vm.cpu.read_pc();
        let (code, value) = match exit {
            VmExit::UnhandledException(e) => e,
            VmExit::Breakpoint if pc == until => break,
            other => {
                error = Some(format!("{other:?}"));
                break;
            }
        };
        let mut s = st.borrow_mut();
        if code == ExceptionCode::Environment && value == HALT {
            break;
        }
        if code == ExceptionCode::Environment && value == REDIRECT {
            vm.cpu.block_id = u64::MAX;
            vm.cpu.block_offset = 0;
            vm.cpu.exception = Exception::none();
            continue;
        }
        if s.stop.is_some() {
            break;
        }
        let cpu = &mut vm.cpu;
        // An event to ask the client about, or how the run ends.
        let ev = match code {
            ExceptionCode::Syscall => match isa {
                Isa::X86_64 => match x86_trap(&code_at(cpu, pc), value) {
                    Ok(name) => json!({ "event": "syscall", "insn": name, "pc": pc }),
                    Err(n) => json!({ "event": "interrupt", "intno": n, "pc": pc }),
                },
                Isa::Aarch64 => {
                    let w = code_at(cpu, pc).get(..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
                    json!({ "event": "interrupt", "intno": a64_intno(w), "pc": pc })
                }
            },
            // A64 division never traps, but icicle raises this for SDIV of MIN by -1.
            ExceptionCode::DivisionException if isa == Isa::Aarch64 => {
                let w = code_at(cpu, pc).get(..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
                match w.filter(|w| w & 0x7fe0_fc00 == 0x1ac0_0c00) {
                    Some(w) => {
                        a64_sdiv(cpu, &regs, w);
                        cpu.write_pc(pc + 4);
                        cpu.block_id = u64::MAX;
                        cpu.block_offset = 0;
                        cpu.exception = Exception::none();
                        continue;
                    }
                    None => {
                        error = Some(format!("DivisionException at 0x{pc:x}"));
                        break;
                    }
                }
            }
            ExceptionCode::DivisionException => json!({ "event": "interrupt", "intno": 0, "pc": pc }),
            ExceptionCode::InvalidInstruction | ExceptionCode::UnimplementedOp if isa == Isa::Aarch64 => {
                let w = code_at(cpu, pc).get(..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
                json!({ "event": "interrupt", "intno": a64_intno(w), "pc": pc })
            }
            ExceptionCode::InvalidInstruction => {
                s.set_stop(json!({ "kind": "undecodable", "pc": pc }));
                break;
            }
            ExceptionCode::ExecViolation if value == until => break,
            ExceptionCode::ExecViolation => {
                let from = s.last_pc();
                s.set_stop(json!({ "kind": "left_code", "from": from, "target": value }));
                break;
            }
            ExceptionCode::ReadUnmapped | ExceptionCode::ReadPerm | ExceptionCode::ReadUnaligned | ExceptionCode::ReadUninitialized | ExceptionCode::WriteUnmapped | ExceptionCode::WritePerm | ExceptionCode::WriteUnaligned => {
                let write = matches!(code, ExceptionCode::WriteUnmapped | ExceptionCode::WritePerm | ExceptionCode::WriteUnaligned);
                let (addr, size, write, at) = s.last_access.unwrap_or((value, 0, write, pc));
                s.set_stop(json!({ "kind": "fault", "write": write, "addr": addr, "size": size, "pc": at }));
                break;
            }
            ExceptionCode::SelfModifyingCode => {
                error = Some("SELF_MODIFYING_CODE".into());
                break;
            }
            ExceptionCode::UnimplementedOp => {
                error = Some("UNIMPLEMENTED_OP".into());
                break;
            }
            other => {
                error = Some(format!("{other:?} (0x{value:x})"));
                break;
            }
        };
        if !event(cpu, &mut s, &regs, ev) {
            break;
        }
        // Resume after the trapping instruction, unless the client moved the pc.
        if cpu.read_pc() == pc {
            let len = match isa {
                Isa::X86_64 => s.lens.get(&pc).copied().unwrap_or(2) as u64,
                Isa::Aarch64 => 4,
            };
            cpu.write_pc(pc + len);
        }
        cpu.block_id = u64::MAX;
        cpu.block_offset = 0;
        cpu.exception = Exception::none();
    }
    let elapsed = started.elapsed().as_millis() as u64;
    let pc = vm.cpu.read_pc();
    let sp = vm.cpu.read_reg(regs.sp);
    let mut s = st.borrow_mut();
    let end = json!({
        "event": "end",
        "stop": s.stop,
        "error": error,
        "pc": pc,
        "sp": sp,
        "count": s.count,
        "elapsed_ms": elapsed,
        "ring": s.ring.iter().map(|(a, n)| json!([a, n])).collect::<Vec<_>>(),
    });
    s.io.send(&end);
    serve(&mut vm.cpu, &mut s, &regs, false);
    Ok(())
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("mukoz-emu-icicle {} ({PROTOCOL}; {ENGINE})", env!("CARGO_PKG_VERSION"));
        return;
    }
    let io = Io { input: std::io::stdin().lock(), out: std::io::stdout().lock() };
    let st = Rc::new(RefCell::new(State {
        io,
        stop: None,
        count: 0,
        ring: VecDeque::with_capacity(RING),
        breaks: HashSet::new(),
        lens: HashMap::new(),
        allowed: Vec::new(),
        code: Vec::new(),
        frozen: Vec::new(),
        until: 0,
        insn_limit: 0,
        deadline: Instant::now(),
        ticks: 0,
        last_access: None,
    }));
    let mut machines: Option<Result<(Machine, Machine), String>> = None;
    loop {
        let m = st.borrow_mut().io.recv();
        match m["op"].as_str().unwrap_or("") {
            "hello" => st.borrow_mut().io.send(&json!({ "protocol": PROTOCOL, "program": env!("CARGO_PKG_NAME"), "engine": ENGINE, "version": env!("CARGO_PKG_VERSION") })),
            "run" => {
                let isa = match m["isa"].as_str() {
                    Some("x86_64") => Isa::X86_64,
                    Some("aarch64") => Isa::Aarch64,
                    _ => {
                        st.borrow_mut().io.send(&json!({ "event": "setup_error", "error": "isa must be x86_64 or aarch64" }));
                        continue;
                    }
                };
                let built = machines.get_or_insert_with(|| build_all(&st));
                let r = match built {
                    Ok((x86, a64)) => run(if isa == Isa::X86_64 { x86 } else { a64 }, &st, isa, &m),
                    Err(e) => Err(format!("engine: {e}")),
                };
                if let Err(e) = r {
                    st.borrow_mut().io.send(&json!({ "event": "setup_error", "error": e }));
                }
            }
            "quit" => return,
            other => st.borrow_mut().io.send(&json!({ "error": format!("unknown op `{other}`") })),
        }
    }
}
