//! Emulated executor on Unicorn: routines (docs/05 5.3-5.5) and Linux processes
//! with a modeled system-call interface and boundary monitors (docs/13).
//!
//! One fresh engine instance per case: re-using an instance after rewriting
//! code was observed to execute stale translated blocks (docs/devlog.md).

use crate::expr::{self, EvalCtx, Ty, ValEnv, Value};
use crate::image::{DATA_BASE, Image, Monitor};
use crate::plan::{Case, SplitMix64};
use crate::spec::{Access, Binding, Contract, Isa};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use unicorn_engine::unicorn_const::{Arch, HookType, MemType, Mode, Permission, uc_error};
use unicorn_engine::{InsnSysX86, RegisterARM64, RegisterX86, Unicorn};

pub const ENGINE: &str = "unicorn-engine 2.1.1 (bundled C, crate pin)";
pub const STACK_TOP: u64 = 0x7fff_0000;
pub const REGION_BASE: u64 = 0x2000_0000;
pub const REGION_STRIDE: u64 = 0x0100_0000;
pub const SENTINEL: u64 = 0x0dea_d000;
const PAGE: u64 = 0x1000;
const RING: usize = 64;
const SYSCALL_LOG: usize = 16;
const DEFAULT_STREAM_CAP: usize = 1 << 16;
const MAX_FDS: usize = 16;

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

fn sp_id(isa: Isa) -> i32 {
    match isa {
        Isa::X86_64 => RegisterX86::RSP.into(),
        Isa::Aarch64 => RegisterARM64::SP.into(),
    }
}

// ---------------------------------------------------------------- results

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stop {
    Returned,
    /// Reached the return sentinel with the wrong stack pointer.
    BadReturn { sp: String, expected_sp: String },
    /// A process called exit / exit_group.
    Exited { status: u64 },
    /// Memory access outside the allowed regions.
    MemoryViolation { pc_offset: String, access: String, address: String, size: usize, detail: String },
    /// Control left the code region.
    LeftCode { from_offset: Option<String>, target: String },
    InvalidInstruction { pc_offset: String },
    ForbiddenEffect { pc_offset: String, effect: String },
    /// A system call the effect model does not implement (not judged as wrong).
    UnsupportedSyscall { pc_offset: String, number: u64 },
    /// stdout/stderr grew beyond the contract's max_len.
    OutputLimit { pc_offset: String, stream: String, limit: usize },
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

/// What one boundary monitor saw during one case.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct MonStats {
    pub calls: u64,
    pub returns: u64,
    pub requires_violations: u64,
    pub ensures_violations: u64,
    pub abi_violations: u64,
    pub inconclusive: u64,
    /// First few violations with detail.
    pub details: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Default)]
pub struct ProcOut {
    pub exit_status: Option<u64>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// File name (binding key) → (exists, contents).
    pub files: BTreeMap<String, (bool, Vec<u8>)>,
    pub syscalls: Vec<String>,
    pub syscall_count: u64,
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
    pub process: Option<ProcOut>,
    pub monitors: BTreeMap<String, MonStats>,
}

pub struct PlacedRegion {
    pub name: String,
    pub addr: u64,
    pub size: u64,
    pub access: Access,
}

// ---------------------------------------------------------------- process model

#[derive(Clone)]
enum FdKind {
    Stdin,
    Stdout,
    Stderr,
    File(String),
}

#[derive(Clone)]
struct Fd {
    kind: FdKind,
    pos: u64,
    read: bool,
    write: bool,
    append: bool,
}

struct VFile {
    name: String,
    exists: bool,
    data: Vec<u8>,
    cap: usize,
}

struct Proc {
    stdin: Vec<u8>,
    stdin_pos: usize,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    out_cap: usize,
    err_cap: usize,
    files: BTreeMap<String, VFile>,
    fds: Vec<Option<Fd>>,
    allow: Vec<String>,
    exit: Option<u64>,
    log: Vec<String>,
    count: u64,
}

#[derive(Clone, Copy)]
enum Sys {
    Read,
    Write,
    Open,
    OpenAt,
    Close,
    Lseek,
    Exit,
}

/// darwin-stdio/1: read, write, close and exit (BSD numbers; x86_64 adds the 0x2000000 class).
/// open is not modeled (its flag values differ from Linux) and is reported as unsupported.
fn sys_of_darwin(isa: Isa, nr: u64) -> Option<Sys> {
    let n = match isa {
        Isa::X86_64 => nr.checked_sub(0x200_0000)?,
        Isa::Aarch64 => nr,
    };
    Some(match n {
        1 => Sys::Exit,
        3 => Sys::Read,
        4 => Sys::Write,
        6 => Sys::Close,
        _ => return None,
    })
}

fn sys_of(isa: Isa, nr: u64) -> Option<Sys> {
    Some(match (isa, nr) {
        (Isa::X86_64, 0) | (Isa::Aarch64, 63) => Sys::Read,
        (Isa::X86_64, 1) | (Isa::Aarch64, 64) => Sys::Write,
        (Isa::X86_64, 2) => Sys::Open,
        (Isa::X86_64, 257) | (Isa::Aarch64, 56) => Sys::OpenAt,
        (Isa::X86_64, 3) | (Isa::Aarch64, 57) => Sys::Close,
        (Isa::X86_64, 8) | (Isa::Aarch64, 62) => Sys::Lseek,
        (Isa::X86_64, 60) | (Isa::X86_64, 231) | (Isa::Aarch64, 93) | (Isa::Aarch64, 94) => Sys::Exit,
        _ => return None,
    })
}

const ENOENT: u64 = 2;
const EBADF: u64 = 9;
const EACCES: u64 = 13;
const EEXIST: u64 = 17;
const EINVAL: u64 = 22;
const EMFILE: u64 = 24;
const ENOSPC: u64 = 28;
const ESPIPE: u64 = 29;

fn neg(e: u64) -> u64 {
    (-(e as i64)) as u64
}

type Allowed = Rc<Vec<(u64, u64, bool, bool, String)>>;

fn range_ok(allowed: &[(u64, u64, bool, bool, String)], addr: u64, len: u64, write: bool) -> bool {
    if len == 0 {
        return true;
    }
    let Some(end) = addr.checked_add(len) else { return false };
    allowed.iter().any(|(lo, hi, r, w, _)| addr >= *lo && end <= *hi && if write { *w } else { *r })
}

// ---------------------------------------------------------------- monitors

struct Frame {
    mon: usize,
    ret: u64,
    sp_after: u64,
    saved: Vec<(&'static str, u64)>,
    env: ValEnv,
    regions: Vec<(usize, u64, u64)>,
    /// The callee's requires held (and were decidable) at the call: only then is it bound by
    /// its ensures. A call that breaks requires is the caller's fault alone.
    obligated: bool,
}

struct MonState {
    frames: Vec<Frame>,
    stats: BTreeMap<String, MonStats>,
}

fn note(stats: &mut MonStats, v: serde_json::Value) {
    if stats.details.len() < 3 {
        stats.details.push(v);
    }
}

fn eval_conds(conds: &[crate::spec::Cond], env: &ValEnv) -> Vec<(String, Result<bool, String>, Vec<(String, String)>)> {
    let empty = BTreeMap::new();
    let cx = EvalCtx { vars: env, region_addrs: &empty };
    conds
        .iter()
        .map(|c| match expr::eval(&c.expr, &cx) {
            Ok(Value::Bool(true)) => (c.id.clone(), Ok(true), Vec::new()),
            Ok(_) => (c.id.clone(), Ok(false), expr::explain_false(&c.expr, &cx)),
            Err(m) => (c.id.clone(), Err(m), Vec::new()),
        })
        .collect()
}

fn monitor_enter(uc: &mut Unicorn<()>, isa: Isa, m: &Monitor, idx: usize, ms: &mut MonState, loc: &dyn Fn(u64) -> String) {
    // Return address and the stack pointer expected after the return.
    let sp = uc.reg_read(sp_id(isa)).unwrap_or(0);
    let (ret, sp_after) = match isa {
        Isa::X86_64 => {
            let mut b = [0u8; 8];
            let _ = uc.mem_read(sp, &mut b);
            (u64::from_le_bytes(b), sp + 8)
        }
        Isa::Aarch64 => (uc.reg_read(RegisterARM64::X30).unwrap_or(0), sp),
    };
    let st = ms.stats.entry(m.symbol.clone()).or_default();
    st.calls += 1;
    let mut env = ValEnv::new();
    let mut lens: BTreeMap<String, u64> = BTreeMap::new();
    let mut addrs: BTreeMap<String, u64> = BTreeMap::new();
    for (reg, src) in &m.args {
        let v = uc.reg_read(reg_id(isa, reg)).unwrap_or(0);
        match src {
            crate::image::ArgSrc::Value(k, Ty::Bv(w)) => {
                env.insert(k.clone(), Value::bv(*w, v));
            }
            crate::image::ArgSrc::Value(k, _) => {
                env.insert(k.clone(), Value::Bool(v & 1 == 1));
            }
            crate::image::ArgSrc::Len(k) => {
                lens.insert(k.clone(), v);
            }
            crate::image::ArgSrc::Addr(r) => {
                addrs.insert(r.clone(), v);
            }
        }
    }
    let mut regions = Vec::new();
    for (i, rp) in m.regions.iter().enumerate() {
        let size = if let Some(k) = &rp.size_from_len {
            lens[k]
        } else {
            let empty = BTreeMap::new();
            match rp.size_expr.as_ref().map(|e| expr::eval(e, &EvalCtx { vars: &env, region_addrs: &empty })) {
                Some(Ok(Value::Bv(_, n))) => n,
                other => {
                    st.inconclusive += 1;
                    note(st, serde_json::json!({ "kind": "monitor_error", "symbol": m.symbol, "error": format!("region {} size: {other:?}", rp.name) }));
                    return;
                }
            }
        };
        let addr = addrs[&rp.name];
        if size > 1 << 20 {
            st.requires_violations += 1;
            note(st, serde_json::json!({ "kind": "requires", "symbol": m.symbol, "detail": format!("region {} of {size} bytes passed (limit 1 MiB)", rp.name) }));
            return;
        }
        let bytes = match uc.mem_read_as_vec(addr, size as usize) {
            Ok(b) => b,
            Err(_) => {
                st.requires_violations += 1;
                note(st, serde_json::json!({ "kind": "requires", "symbol": m.symbol, "detail": format!("region {} = [0x{addr:x}, +{size}) is not mapped memory", rp.name) }));
                return;
            }
        };
        if let Some(v) = &rp.var {
            env.insert(v.clone(), Value::Bytes(bytes));
        }
        regions.push((i, addr, size));
    }
    let mut ok = true;
    for (id, r, why) in eval_conds(&m.contract.requires, &env) {
        match r {
            Ok(true) => {}
            Ok(false) => {
                ok = false;
                st.requires_violations += 1;
                note(st, serde_json::json!({ "kind": "requires", "symbol": m.symbol, "requires": id, "return_to": loc(ret), "why_false": why.into_iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>() }));
            }
            Err(e) => {
                ok = false;
                st.inconclusive += 1;
                note(st, serde_json::json!({ "kind": "monitor_error", "symbol": m.symbol, "requires": id, "error": e }));
            }
        }
    }
    let saved = isa_info(isa).callee_saved.iter().map(|r| (*r, uc.reg_read(reg_id(isa, r)).unwrap_or(0))).collect();
    ms.frames.push(Frame { mon: idx, ret, sp_after, saved, env, regions, obligated: ok });
}

fn monitor_return(uc: &mut Unicorn<()>, isa: Isa, m: &Monitor, f: Frame, ms: &mut MonState) {
    let st = ms.stats.entry(m.symbol.clone()).or_default();
    st.returns += 1;
    let mut env = f.env;
    for (name, reg) in &m.binding.results {
        let raw = uc.reg_read(reg_id(isa, reg)).unwrap_or(0);
        let v = match m.contract.results.get(name).map(|t| t.ty) {
            Some(Ty::Bv(w)) => Value::bv(w, raw),
            _ => Value::Bool(raw & 1 == 1),
        };
        env.insert(format!("result.{name}"), v);
    }
    for (i, addr, size) in &f.regions {
        if let Some(o) = &m.regions[*i].observe {
            let b = uc.mem_read_as_vec(*addr, *size as usize).unwrap_or_default();
            env.insert(format!("after.{o}"), Value::Bytes(b));
        }
    }
    // ABI preservation is owed on every call; ensures and frame only when requires held.
    let conds: &[crate::spec::Cond] = if f.obligated { &m.contract.ensures } else { &[] };
    for (id, r, why) in eval_conds(conds, &env) {
        match r {
            Ok(true) => {}
            Ok(false) => {
                st.ensures_violations += 1;
                note(st, serde_json::json!({ "kind": "ensures", "symbol": m.symbol, "ensures": id, "why_false": why.into_iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>() }));
            }
            Err(e) => {
                st.inconclusive += 1;
                note(st, serde_json::json!({ "kind": "monitor_error", "symbol": m.symbol, "ensures": id, "error": e }));
            }
        }
    }
    for s in m.contract.state.keys().filter(|_| f.obligated) {
        if !m.contract.modifies.contains(s) {
            if let (Some(b), Some(a)) = (env.get(&format!("before.{s}")), env.get(&format!("after.{s}"))) {
                if b != a {
                    st.ensures_violations += 1;
                    note(st, serde_json::json!({ "kind": "ensures", "symbol": m.symbol, "ensures": format!("frame.{s}"), "detail": "state not in `modifies` changed" }));
                }
            }
        }
    }
    let changed: Vec<String> = f
        .saved
        .iter()
        .filter_map(|(r, v)| {
            let now = uc.reg_read(reg_id(isa, r)).unwrap_or(0);
            (now != *v).then(|| format!("{r}: 0x{v:x} -> 0x{now:x}"))
        })
        .collect();
    if !changed.is_empty() {
        st.abi_violations += 1;
        note(st, serde_json::json!({ "kind": "abi", "symbol": m.symbol, "changed_registers": changed }));
    }
}

// ---------------------------------------------------------------- execution

struct HookState {
    stop: Option<Stop>,
    count: u64,
    ring: Vec<(u64, u32)>,
}

/// One binding region as placed for a routine case. The same placement is used by every
/// executor so that their results can be compared (docs/06 6.8).
pub struct RegionSetup {
    pub name: String,
    pub map_base: u64,
    /// Mapped bytes; the last page is past the region (a guard page for native execution).
    pub map_size: u64,
    pub addr: u64,
    pub size: u64,
    pub access: Access,
    pub init: Vec<u8>,
}

pub struct RoutineSetup {
    pub regions: Vec<RegionSetup>,
    pub regs_in: BTreeMap<String, u64>,
}

/// Region placement and initial registers of a routine case, derived only from the binding and
/// the case (values and filler seed).
pub fn routine_setup(binding: &Binding, case: &Case, vary_placement: bool) -> Result<RoutineSetup, String> {
    let info = isa_info(binding.target.isa);
    let empty = BTreeMap::new();
    let mut filler = SplitMix64::new(case.filler_seed);
    let mut placement = SplitMix64::new(case.filler_seed ^ 0x9e37_79b9_7f4a_7c15);
    let mut regions = Vec::new();
    let mut addrs: BTreeMap<String, u64> = BTreeMap::new();
    for (i, r) in binding.regions.iter().enumerate() {
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
            Some(e) => bytes_of(expr::eval(e, &cx).map_err(|m| format!("region {} init: {m}", r.name))?),
            None => (0..size).map(|_| filler.next() as u8).collect(),
        };
        if init.len() as u64 != size {
            return Err(format!("BINDING_MISMATCH: region {} init has {} bytes but size is {size}", r.name, init.len()));
        }
        let map_base = REGION_BASE + i as u64 * REGION_STRIDE;
        let map_size = page_up(size.max(1) + 16) + PAGE;
        let addr = if vary_placement {
            let pad = placement.next() % 16;
            map_base + map_size - PAGE - size - pad
        } else {
            map_base + map_size - PAGE - size.div_ceil(16) * 16
        };
        addrs.insert(r.name.clone(), addr);
        regions.push(RegionSetup { name: r.name.clone(), map_base, map_size, addr, size, access: r.access, init });
    }
    let mut regs_in = BTreeMap::new();
    for g in info.gprs {
        regs_in.insert(g.to_string(), filler.next());
    }
    let cx = EvalCtx { vars: &case.values, region_addrs: &addrs };
    for (reg, e) in &binding.arguments {
        let v = match expr::eval(e, &cx).map_err(|m| format!("argument {reg}: {m}"))? {
            Value::Bv(w, x) if w < 64 => x | (filler.next() & !expr::mask(w)),
            Value::Bv(_, x) => x,
            Value::Bool(b) => b as u64,
            Value::Bytes(_) => return Err(format!("argument {reg} evaluated to bytes")),
        };
        regs_in.insert(reg.clone(), v);
    }
    Ok(RoutineSetup { regions, regs_in })
}

pub struct Executor<'a> {
    pub image: &'a Image,
    pub contract: &'a Contract,
    pub binding: &'a Binding,
    pub insn_limit: u64,
    pub timeout_ms: u64,
    /// Vary each region's start alignment per case (derived from the case seed, so replays match).
    pub vary_placement: bool,
}

fn setup_err(e: String) -> Observation {
    Observation {
        stop: Stop::SetupError { error: e },
        regs_in: BTreeMap::new(),
        regs_out: BTreeMap::new(),
        flags_out: 0,
        regions_out: BTreeMap::new(),
        recent: Vec::new(),
        instructions: 0,
        process: None,
        monitors: BTreeMap::new(),
    }
}

fn bytes_of(v: Value) -> Vec<u8> {
    match v {
        Value::Bytes(b) => b,
        Value::Bv(w, x) => x.to_le_bytes()[..(w / 8) as usize].to_vec(),
        Value::Bool(b) => vec![b as u8],
    }
}

impl Executor<'_> {
    pub fn run(&self, case: &Case) -> Observation {
        self.run_inner(case).unwrap_or_else(setup_err)
    }

    fn stream_cap(&self, which: &str) -> usize {
        self.binding
            .results
            .iter()
            .find(|(_, src)| src == which)
            .and_then(|(name, _)| self.contract.results.get(name))
            .map(|v| v.max_len as usize)
            .unwrap_or(DEFAULT_STREAM_CAP)
    }

    fn run_inner(&self, case: &Case) -> Result<Observation, String> {
        let isa = self.binding.target.isa;
        let process = self.binding.target.is_process();
        let info = isa_info(isa);
        let apple = self.binding.target.abi == "apple-arm64";
        let darwin = self.binding.target.os == "darwin";
        let image = self.image;
        let ue = |e: uc_error| format!("engine: {e:?}");
        let mut uc = match isa {
            Isa::X86_64 => Unicorn::new(Arch::X86, Mode::MODE_64),
            Isa::Aarch64 => Unicorn::new(Arch::ARM64, Mode::ARM),
        }
        .map_err(ue)?;
        let loc = {
            let image = image.clone();
            move |a: u64| image.locate(a)
        };

        // Image segments (+ the data area of a raw process). Pages are mapped with every
        // permission; the access hooks below enforce each segment's own permissions.
        let mut segments = image.segments.clone();
        if let (true, Some(p)) = (process && self.binding.target.format == crate::spec::Format::Raw, &self.binding.process) {
            if p.data_bytes > 0 {
                segments.push(crate::image::Segment { addr: DATA_BASE, data: Vec::new(), mem_size: p.data_bytes, read: true, write: true, exec: false, label: "data area".into() });
            }
        }
        let mut pages: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
        for s in &segments {
            let lo = s.addr / PAGE * PAGE;
            let hi = page_up(s.addr + s.mem_size.max(1));
            let mut a = lo;
            while a < hi {
                pages.insert(a);
                a += PAGE;
            }
        }
        let mut run_start: Option<(u64, u64)> = None;
        for p in pages.iter().copied().chain(std::iter::once(u64::MAX)) {
            match run_start {
                Some((lo, hi)) if hi == p => run_start = Some((lo, hi + PAGE)),
                Some((lo, hi)) => {
                    uc.mem_map(lo, (hi - lo) as usize, Permission::ALL).map_err(ue)?;
                    run_start = if p == u64::MAX { None } else { Some((p, p + PAGE)) };
                }
                None if p != u64::MAX => run_start = Some((p, p + PAGE)),
                None => {}
            }
        }
        for s in &segments {
            if !s.data.is_empty() {
                uc.mem_write(s.addr, &s.data).map_err(ue)?;
            }
        }

        // Stack.
        let stack_size = page_up(self.binding.stack_bytes.max(PAGE));
        let stack_lo = STACK_TOP - stack_size;
        uc.mem_map(stack_lo, stack_size as usize, Permission::READ | Permission::WRITE).map_err(ue)?;

        let empty = BTreeMap::new();
        let mut addrs: BTreeMap<String, u64> = BTreeMap::new();
        let mut placed: Vec<PlacedRegion> = Vec::new();
        let mut regs_in: BTreeMap<String, u64> = BTreeMap::new();
        let entry_sp;
        let mut proc_state: Option<Proc> = None;

        if !process {
            // Regions: values from the case, addresses fixed per region index.
            let rs = routine_setup(self.binding, case, self.vary_placement)?;
            for r in &rs.regions {
                uc.mem_map(r.map_base, r.map_size as usize, Permission::READ | Permission::WRITE).map_err(ue)?;
            }
            for r in &rs.regions {
                uc.mem_write(r.addr, &r.init).map_err(ue)?;
                addrs.insert(r.name.clone(), r.addr);
                placed.push(PlacedRegion { name: r.name.clone(), addr: r.addr, size: r.size, access: r.access });
            }
            regs_in = rs.regs_in;
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
        } else {
            // Linux process start: registers zero, sp -> argc, argv[], NULL, envp NULL, auxv AT_NULL.
            let p = self.binding.process.as_ref().ok_or("process binding without [process]")?;
            let cx = EvalCtx { vars: &case.values, region_addrs: &empty };
            let mut argv: Vec<Vec<u8>> = vec![p.argv0.as_bytes().to_vec()];
            let argc_used = match &p.argc {
                Some(e) => match expr::eval(e, &cx).map_err(|m| format!("process.argc: {m}"))? {
                    Value::Bv(_, n) if n as usize <= p.argv.len() => n as usize,
                    Value::Bv(_, n) => {
                        return Err(format!("BINDING_MISMATCH: process.argc = {n} but only {} argv expressions are bound; limit the generator (e.g. max) or add argv entries", p.argv.len()))
                    }
                    _ => return Err("process.argc is not a bitvector".into()),
                },
                None => p.argv.len(),
            };
            for (i, e) in p.argv.iter().take(argc_used).enumerate() {
                let b = bytes_of(expr::eval(e, &cx).map_err(|m| format!("process.argv[{i}]: {m}"))?);
                if b.contains(&0) {
                    return Err(format!("process.argv[{i}] contains a NUL byte; generated arguments must not contain 0x00"));
                }
                argv.push(b);
            }
            let stdin = match &p.stdin {
                Some(e) => bytes_of(expr::eval(e, &cx).map_err(|m| format!("process.stdin: {m}"))?),
                None => Vec::new(),
            };
            let mut files = BTreeMap::new();
            for f in &self.binding.files {
                let data = match &f.init {
                    Some(e) => bytes_of(expr::eval(e, &cx).map_err(|m| format!("file {} init: {m}", f.name))?),
                    None => Vec::new(),
                };
                let exists = match &f.exists {
                    Some(e) => matches!(expr::eval(e, &cx).map_err(|m| format!("file {} exists: {m}", f.name))?, Value::Bool(true)),
                    None => true,
                };
                let cap = f.observe_state.as_ref().and_then(|s| self.contract.state.get(s)).map(|v| v.max_len as usize).unwrap_or(DEFAULT_STREAM_CAP);
                files.insert(f.path.clone(), VFile { name: f.name.clone(), exists, data: if exists { data } else { Vec::new() }, cap });
            }
            // Strings at the top of the stack, then the pointer block 16-byte aligned.
            let mut top = STACK_TOP - 16;
            let mut ptrs = Vec::new();
            for a in &argv {
                top -= a.len() as u64 + 1;
                let mut z = a.clone();
                z.push(0);
                uc.mem_write(top, &z).map_err(ue)?;
                ptrs.push(top);
            }
            let words = 1 + ptrs.len() + 1 + 1 + 2;
            let sp = (top - 64 - words as u64 * 8) & !15;
            let mut block: Vec<u8> = Vec::new();
            block.extend_from_slice(&(ptrs.len() as u64).to_le_bytes());
            for a in &ptrs {
                block.extend_from_slice(&a.to_le_bytes());
            }
            block.extend_from_slice(&[0u8; 8 * 4]); // argv NULL, envp NULL, AT_NULL pair
            uc.mem_write(sp, &block).map_err(ue)?;
            for g in info.gprs {
                regs_in.insert(g.to_string(), 0);
            }
            let mut sp = sp;
            if image.main_call {
                // LC_MAIN: call main(argc, argv, envp, apple); its return goes to the sentinel.
                let argv_at = sp + 8;
                let envp_at = argv_at + 8 * ptrs.len() as u64 + 8;
                let apple_at = envp_at + 8;
                let regs: [&str; 4] = match isa {
                    Isa::X86_64 => ["rdi", "rsi", "rdx", "rcx"],
                    Isa::Aarch64 => ["x0", "x1", "x2", "x3"],
                };
                for (r, v) in regs.iter().zip([ptrs.len() as u64, argv_at, envp_at, apple_at]) {
                    regs_in.insert(r.to_string(), v);
                }
                match isa {
                    Isa::X86_64 => {
                        sp -= 8;
                        uc.mem_write(sp, &SENTINEL.to_le_bytes()).map_err(ue)?;
                    }
                    Isa::Aarch64 => {
                        uc.reg_write(RegisterARM64::X30, SENTINEL).map_err(ue)?;
                    }
                }
            }
            entry_sp = sp;
            uc.reg_write(sp_id(isa), sp).map_err(ue)?;
            if isa == Isa::X86_64 {
                uc.reg_write(RegisterX86::RFLAGS, 0x202).map_err(ue)?;
            }
            proc_state = Some(Proc {
                stdin,
                stdin_pos: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                out_cap: self.stream_cap("stdout"),
                err_cap: self.stream_cap("stderr"),
                files,
                fds: vec![
                    Some(Fd { kind: FdKind::Stdin, pos: 0, read: true, write: false, append: false }),
                    Some(Fd { kind: FdKind::Stdout, pos: 0, read: false, write: true, append: true }),
                    Some(Fd { kind: FdKind::Stderr, pos: 0, read: false, write: true, append: true }),
                ],
                allow: self.contract.effects.clone(),
                exit: None,
                log: Vec::new(),
                count: 0,
            });
        }
        for (r, v) in &regs_in {
            uc.reg_write(reg_id(isa, r), *v).map_err(ue)?;
        }

        // Allowed data accesses: (lo, hi, read, write, label).
        let stack_hi = if process {
            STACK_TOP
        } else {
            match isa {
                Isa::X86_64 => entry_sp + 8,
                Isa::Aarch64 => entry_sp,
            }
        };
        let mut allowed: Vec<(u64, u64, bool, bool, String)> = Vec::new();
        for s in &segments {
            allowed.push((s.addr, s.addr + s.mem_size, s.read, s.write, s.label.clone()));
        }
        allowed.push((stack_lo, stack_hi, true, true, if process { "stack".into() } else { "stack (below the entry stack pointer)".into() }));
        for p in &placed {
            let (r, w) = match p.access {
                Access::R => (true, false),
                Access::W => (false, true),
                Access::Rw => (true, true),
            };
            allowed.push((p.addr, p.addr + p.size, r, w, format!("region `{}`", p.name)));
        }
        let allowed: Allowed = Rc::new(allowed);

        let st = Rc::new(RefCell::new(HookState { stop: None, count: 0, ring: Vec::with_capacity(RING) }));
        let code_ranges = image.code_ranges.clone();
        let proc_rc = Rc::new(RefCell::new(proc_state));
        let mon = Rc::new(RefCell::new(MonState { frames: Vec::new(), stats: BTreeMap::new() }));
        let monitors: Rc<Vec<Monitor>> = Rc::new(image.monitors.clone());
        let mon_at: Rc<HashMap<u64, usize>> = Rc::new(monitors.iter().enumerate().map(|(i, m)| (m.addr, i)).collect());

        // Instruction hook: ring buffer, budget accounting, code range check, monitors.
        {
            let st = st.clone();
            let x18_in = regs_in.get("x18").copied();
            let x18 = if apple { Some(reg_id(isa, "x18")) } else { None };
            let loc = loc.clone();
            let mon = mon.clone();
            let monitors = monitors.clone();
            let mon_at = mon_at.clone();
            uc.add_code_hook(0, u64::MAX, move |uc, addr, size| {
                let mut s = st.borrow_mut();
                if s.stop.is_some() {
                    return;
                }
                // Unicorn reports an undecodable instruction with this size marker.
                if size == 0xf1f1_f1f1 {
                    s.stop = Some(Stop::InvalidInstruction { pc_offset: loc(addr) });
                    drop(s);
                    let _ = uc.emu_stop();
                    return;
                }
                if !code_ranges.iter().any(|(lo, hi)| addr >= *lo && addr + size as u64 <= *hi) {
                    let from = s.ring.last().map(|(a, _)| loc(*a));
                    s.stop = Some(Stop::LeftCode { from_offset: from, target: loc(addr) });
                    drop(s);
                    let _ = uc.emu_stop();
                    return;
                }
                if let (Some(id), Some(v0)) = (x18, x18_in) {
                    if uc.reg_read(id).unwrap_or(v0) != v0 {
                        let at = s.ring.last().map(|(a, _)| loc(*a)).unwrap_or_default();
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
                drop(s);
                if !monitors.is_empty() {
                    let mut ms = mon.borrow_mut();
                    let sp = uc.reg_read(sp_id(isa)).unwrap_or(0);
                    while let Some(top) = ms.frames.last() {
                        if top.ret == addr && top.sp_after == sp {
                            let f = ms.frames.pop().unwrap();
                            let m = &monitors[f.mon];
                            monitor_return(uc, isa, m, f, &mut ms);
                        } else {
                            break;
                        }
                    }
                    if let Some(&i) = mon_at.get(&addr) {
                        monitor_enter(uc, isa, &monitors[i], i, &mut ms, &loc);
                    }
                }
            })
            .map_err(ue)?;
        }
        // Data access monitor.
        {
            let st = st.clone();
            let allowed = allowed.clone();
            let loc = loc.clone();
            uc.add_mem_hook(HookType::MEM_READ | HookType::MEM_WRITE, 0, u64::MAX, move |uc, t, addr, size, _v| {
                let write = matches!(t, MemType::WRITE);
                if !range_ok(&allowed, addr, size as u64, write) {
                    let mut s = st.borrow_mut();
                    if s.stop.is_none() {
                        let pc = uc.pc_read().unwrap_or(0);
                        s.stop = Some(Stop::MemoryViolation {
                            pc_offset: loc(pc),
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
            let loc = loc.clone();
            uc.add_mem_hook(HookType::MEM_INVALID, 0, u64::MAX, move |uc, t, addr, size, _v| {
                let mut s = st.borrow_mut();
                if s.stop.is_none() {
                    let pc = uc.pc_read().unwrap_or(0);
                    s.stop = Some(match t {
                        MemType::FETCH_UNMAPPED | MemType::FETCH_PROT => {
                            let from = s.ring.last().map(|(a, _)| loc(*a));
                            Stop::LeftCode { from_offset: from, target: loc(addr) }
                        }
                        _ => {
                            let write = matches!(t, MemType::WRITE_UNMAPPED | MemType::WRITE_PROT);
                            Stop::MemoryViolation {
                                pc_offset: loc(pc),
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
        // Traps. A64 `svc` is a system call in a process; everything else is a forbidden effect.
        {
            let st = st.clone();
            let proc_rc = proc_rc.clone();
            let allowed = allowed.clone();
            let loc = loc.clone();
            uc.add_intr_hook(move |uc, intno| {
                let pc_insn = st.borrow().ring.last().map(|(a, _)| *a).unwrap_or_else(|| uc.pc_read().unwrap_or(0));
                if process && isa == Isa::Aarch64 && intno == 2 {
                    let stop = syscall(uc, isa, darwin, &mut proc_rc.borrow_mut(), &allowed, &loc(pc_insn));
                    if let Some(stop) = stop {
                        let mut s = st.borrow_mut();
                        if s.stop.is_none() {
                            s.stop = Some(stop);
                        }
                        drop(s);
                        let _ = uc.emu_stop();
                    }
                    return;
                }
                let mut s = st.borrow_mut();
                if s.stop.is_none() {
                    // A64 EXCP_UDEF (1): the engine raises it both for architecturally undefined
                    // encodings and for instructions it does not implement, so it cannot be
                    // blamed on the subject (docs/01 P5): unsupported, not a forbidden effect.
                    s.stop = Some(if isa == Isa::Aarch64 && intno == 1 {
                        Stop::InvalidInstruction { pc_offset: loc(pc_insn) }
                    } else {
                        Stop::ForbiddenEffect { pc_offset: loc(pc_insn), effect: format!("interrupt/exception {intno} (e.g. svc/int/brk)") }
                    });
                }
                drop(s);
                let _ = uc.emu_stop();
            })
            .map_err(ue)?;
        }
        if isa == Isa::X86_64 {
            for (kind, name) in [(InsnSysX86::SYSCALL, "syscall"), (InsnSysX86::SYSENTER, "sysenter")] {
                let st = st.clone();
                let proc_rc = proc_rc.clone();
                let allowed = allowed.clone();
                let loc = loc.clone();
                uc.add_insn_sys_hook(kind, 0, u64::MAX, move |uc| {
                    let pc = st.borrow().ring.last().map(|(a, _)| *a).unwrap_or(0);
                    if process && name == "syscall" {
                        if let Some(stop) = syscall(uc, isa, darwin, &mut proc_rc.borrow_mut(), &allowed, &loc(pc)) {
                            let mut s = st.borrow_mut();
                            if s.stop.is_none() {
                                s.stop = Some(stop);
                            }
                            drop(s);
                            let _ = uc.emu_stop();
                        }
                        return;
                    }
                    let mut s = st.borrow_mut();
                    if s.stop.is_none() {
                        s.stop = Some(Stop::ForbiddenEffect { pc_offset: loc(pc), effect: name.into() });
                    }
                    drop(s);
                    let _ = uc.emu_stop();
                })
                .map_err(ue)?;
            }
        }

        let started = std::time::Instant::now();
        let res = uc.emu_start(image.entry, SENTINEL, self.timeout_ms * 1000, self.insn_limit as usize);
        let elapsed = started.elapsed();

        let pc = uc.pc_read().unwrap_or(0);
        let sp = uc.reg_read(sp_id(isa)).unwrap_or(0);
        let s = st.borrow();
        let stop = if let Some(stop) = s.stop.clone() {
            stop
        } else {
            match res {
                Ok(()) if pc == SENTINEL && process && image.main_call => {
                    let v = uc.reg_read(match isa {
                        Isa::X86_64 => reg_id(isa, "rax"),
                        Isa::Aarch64 => reg_id(isa, "x0"),
                    })
                    .unwrap_or(0)
                        & 0xff;
                    if let Some(p) = proc_rc.borrow_mut().as_mut() {
                        p.exit = Some(v);
                        push_log(p, format!("return from main ({v})"));
                    }
                    Stop::Exited { status: v }
                }
                Ok(()) if pc == SENTINEL && !process => {
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
                Ok(()) => Stop::EngineError { error: format!("engine stopped at pc {} without a recorded reason", loc(pc)) },
                Err(uc_error::INSN_INVALID) => {
                    let in_code = code_ranges_contains(&image.code_ranges, pc);
                    let at = if in_code { pc } else { s.ring.last().map(|(a, _)| *a).unwrap_or(pc) };
                    Stop::InvalidInstruction { pc_offset: loc(at) }
                }
                Err(e) => Stop::EngineError { error: format!("{e:?} at pc {}", loc(pc)) },
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
            .map(|(a, n)| Insn { offset: loc(*a), bytes: image.code_bytes(*a, *n as usize).map(expr::hex).unwrap_or_default() })
            .collect();
        let process_out = proc_rc.borrow().as_ref().map(|p| ProcOut {
            exit_status: p.exit,
            stdout: p.stdout.clone(),
            stderr: p.stderr.clone(),
            files: p.files.values().map(|f| (f.name.clone(), (f.exists, f.data.clone()))).collect(),
            syscalls: p.log.clone(),
            syscall_count: p.count,
        });
        let monitors_out = mon.borrow().stats.clone();
        Ok(Observation { stop, regs_in, regs_out, flags_out, regions_out, recent, instructions: s.count, process: process_out, monitors: monitors_out })
    }
}

fn code_ranges_contains(r: &[(u64, u64)], a: u64) -> bool {
    r.iter().any(|(lo, hi)| a >= *lo && a < *hi)
}

/// Read a NUL-terminated path (at most 4096 bytes) from readable guest memory.
fn read_path(uc: &Unicorn<()>, allowed: &[(u64, u64, bool, bool, String)], addr: u64) -> Result<Vec<u8>, ()> {
    let mut out = Vec::new();
    for i in 0..4096u64 {
        let a = addr.checked_add(i).ok_or(())?;
        if !range_ok(allowed, a, 1, false) {
            return Err(());
        }
        let mut b = [0u8; 1];
        uc.mem_read(a, &mut b).map_err(|_| ())?;
        if b[0] == 0 {
            return Ok(out);
        }
        out.push(b[0]);
    }
    Err(())
}

/// Perform one system call against the modeled process state. Returns a stop on exit,
/// forbidden or unsupported calls, bad buffers and output overflow.
fn syscall(uc: &mut Unicorn<()>, isa: Isa, darwin: bool, pr: &mut Option<Proc>, allowed: &[(u64, u64, bool, bool, String)], at: &str) -> Option<Stop> {
    let p = pr.as_mut()?;
    let (nr_reg, arg_regs, ret_reg): (&str, [&str; 4], &str) = match (isa, darwin) {
        (Isa::X86_64, _) => ("rax", ["rdi", "rsi", "rdx", "r10"], "rax"),
        (Isa::Aarch64, false) => ("x8", ["x0", "x1", "x2", "x3"], "x0"),
        (Isa::Aarch64, true) => ("x16", ["x0", "x1", "x2", "x3"], "x0"),
    };
    let nr = uc.reg_read(reg_id(isa, nr_reg)).unwrap_or(0);
    let a: Vec<u64> = arg_regs.iter().map(|r| uc.reg_read(reg_id(isa, r)).unwrap_or(0)).collect();
    p.count += 1;
    let sys = if darwin { sys_of_darwin(isa, nr) } else { sys_of(isa, nr) };
    let Some(sys) = sys else {
        return Some(Stop::UnsupportedSyscall { pc_offset: at.to_string(), number: nr });
    };
    let effect = match sys {
        Sys::Read => "read",
        Sys::Write => "write",
        Sys::Open | Sys::OpenAt => "open",
        Sys::Close => "close",
        Sys::Lseek => "lseek",
        Sys::Exit => "exit",
    };
    if effect != "exit" && !p.allow.iter().any(|e| e == effect) {
        return Some(Stop::ForbiddenEffect { pc_offset: at.to_string(), effect: format!("{effect} (not in the contract's effects.allow)") });
    }
    let bad_buf = |addr: u64, len: u64, write: bool, what: &str| Stop::MemoryViolation {
        pc_offset: at.to_string(),
        access: if write { "write".into() } else { "read".into() },
        address: format!("0x{addr:x}"),
        size: len as usize,
        detail: format!("{what}: {}", describe_access(addr, len as usize, write, allowed)),
    };
    let fd_ok = |p: &Proc, fd: u64| -> Option<Fd> { p.fds.get(fd as usize).and_then(|f| f.clone()) };
    let ret: u64 = match sys {
        Sys::Exit => {
            p.exit = Some(a[0] & 0xff);
            push_log(p, format!("exit({})", a[0] & 0xff));
            return Some(Stop::Exited { status: a[0] & 0xff });
        }
        Sys::Read => {
            let (fd, buf, len) = (a[0], a[1], a[2]);
            match fd_ok(p, fd) {
                Some(f) if f.read => {
                    if !range_ok(allowed, buf, len, true) {
                        return Some(bad_buf(buf, len, true, "read(2) buffer"));
                    }
                    let src: &[u8] = match &f.kind {
                        FdKind::Stdin => &p.stdin[p.stdin_pos.min(p.stdin.len())..],
                        FdKind::File(path) => {
                            let d = &p.files[path].data;
                            &d[(f.pos as usize).min(d.len())..]
                        }
                        _ => &[],
                    };
                    let n = (len as usize).min(src.len());
                    let chunk = src[..n].to_vec();
                    let _ = uc.mem_write(buf, &chunk);
                    match &f.kind {
                        FdKind::Stdin => p.stdin_pos += n,
                        _ => p.fds[fd as usize].as_mut().unwrap().pos += n as u64,
                    }
                    n as u64
                }
                _ => neg(EBADF),
            }
        }
        Sys::Write => {
            let (fd, buf, len) = (a[0], a[1], a[2]);
            match fd_ok(p, fd) {
                Some(f) if f.write => {
                    if !range_ok(allowed, buf, len, false) {
                        return Some(bad_buf(buf, len, false, "write(2) buffer"));
                    }
                    let data = uc.mem_read_as_vec(buf, len as usize).unwrap_or_default();
                    match &f.kind {
                        FdKind::Stdout | FdKind::Stderr => {
                            let (out, cap, name) = if matches!(f.kind, FdKind::Stdout) { (&mut p.stdout, p.out_cap, "stdout") } else { (&mut p.stderr, p.err_cap, "stderr") };
                            if out.len() + data.len() > cap {
                                return Some(Stop::OutputLimit { pc_offset: at.to_string(), stream: name.into(), limit: cap });
                            }
                            out.extend_from_slice(&data);
                            len
                        }
                        FdKind::File(path) => {
                            let vf = p.files.get_mut(path).unwrap();
                            let pos = if f.append { vf.data.len() } else { f.pos as usize };
                            let end = pos + data.len();
                            if end > vf.cap {
                                neg(ENOSPC)
                            } else {
                                if vf.data.len() < end {
                                    vf.data.resize(end, 0);
                                }
                                vf.data[pos..end].copy_from_slice(&data);
                                p.fds[fd as usize].as_mut().unwrap().pos = end as u64;
                                len
                            }
                        }
                        FdKind::Stdin => neg(EBADF),
                    }
                }
                _ => neg(EBADF),
            }
        }
        Sys::Open | Sys::OpenAt => {
            let (path_ptr, flags) = if matches!(sys, Sys::Open) { (a[0], a[1]) } else { (a[1], a[2]) };
            let Ok(path) = read_path(uc, allowed, path_ptr) else {
                return Some(bad_buf(path_ptr, 1, false, "open(2) path (unreadable or not NUL-terminated within 4096 bytes)"));
            };
            let path = String::from_utf8_lossy(&path).to_string();
            let acc = flags & 3;
            let (creat, excl, trunc, append) = (flags & 0x40 != 0, flags & 0x80 != 0, flags & 0x200 != 0, flags & 0x400 != 0);
            let fd_slot = (3..MAX_FDS).find(|i| p.fds.get(*i).is_none_or(|f| f.is_none()));
            match p.files.get_mut(&path) {
                None if creat => neg(EACCES),
                None => neg(ENOENT),
                Some(_) if acc == 3 => neg(EINVAL),
                Some(vf) => {
                    if !vf.exists && !creat {
                        neg(ENOENT)
                    } else if vf.exists && creat && excl {
                        neg(EEXIST)
                    } else if let Some(slot) = fd_slot {
                        if !vf.exists {
                            vf.exists = true;
                            vf.data.clear();
                        }
                        if trunc && acc != 0 {
                            vf.data.clear();
                        }
                        if p.fds.len() <= slot {
                            p.fds.resize(slot + 1, None);
                        }
                        p.fds[slot] = Some(Fd { kind: FdKind::File(path.clone()), pos: 0, read: acc == 0 || acc == 2, write: acc == 1 || acc == 2, append });
                        slot as u64
                    } else {
                        neg(EMFILE)
                    }
                }
            }
        }
        Sys::Close => {
            let fd = a[0] as usize;
            if p.fds.get(fd).is_some_and(|f| f.is_some()) {
                p.fds[fd] = None;
                0
            } else {
                neg(EBADF)
            }
        }
        Sys::Lseek => {
            let (fd, off, whence) = (a[0], a[1] as i64, a[2]);
            match fd_ok(p, fd) {
                Some(Fd { kind: FdKind::File(path), pos, .. }) => {
                    let base = match whence {
                        0 => Some(0i64),
                        1 => Some(pos as i64),
                        2 => Some(p.files[&path].data.len() as i64),
                        _ => None,
                    };
                    match base.and_then(|b| b.checked_add(off)) {
                        Some(n) if n >= 0 => {
                            p.fds[fd as usize].as_mut().unwrap().pos = n as u64;
                            n as u64
                        }
                        _ => neg(EINVAL),
                    }
                }
                Some(_) => neg(ESPIPE),
                None => neg(EBADF),
            }
        }
    };
    push_log(p, format!("{effect}({}) = {}", a.iter().take(3).map(|x| format!("0x{x:x}")).collect::<Vec<_>>().join(", "), ret as i64));
    if darwin {
        // darwin-stdio/1: an error sets the carry flag and returns the positive errno.
        let err = (ret as i64) < 0 && (ret as i64) >= -4095;
        let v = if err { (ret as i64).unsigned_abs() } else { ret };
        let _ = uc.reg_write(reg_id(isa, ret_reg), v);
        match isa {
            Isa::Aarch64 => {
                let f = uc.reg_read(RegisterARM64::NZCV).unwrap_or(0);
                let _ = uc.reg_write(RegisterARM64::NZCV, if err { f | 1 << 29 } else { f & !(1 << 29) });
            }
            Isa::X86_64 => {
                let f = uc.reg_read(RegisterX86::RFLAGS).unwrap_or(0);
                let _ = uc.reg_write(RegisterX86::RFLAGS, if err { f | 1 } else { f & !1 });
            }
        }
    } else {
        let _ = uc.reg_write(reg_id(isa, ret_reg), ret);
    }
    None
}

fn push_log(p: &mut Proc, s: String) {
    if p.log.len() == SYSCALL_LOG {
        p.log.remove(0);
    }
    p.log.push(s);
}

fn describe_access(addr: u64, size: usize, write: bool, allowed: &[(u64, u64, bool, bool, String)]) -> String {
    let kind = if write { "write" } else { "read" };
    let end = addr.saturating_add(size as u64);
    for (lo, hi, _r, _w, label) in allowed {
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

/// Values the contract sees after a completed execution (return or exit).
pub fn post_values(binding: &Binding, case: &Case, obs: &Observation, results_ty: &BTreeMap<String, Ty>, state_ty: &BTreeMap<String, Ty>) -> Result<ValEnv, String> {
    let mut env = case.values.clone();
    if let Some(p) = &obs.process {
        for (name, src) in &binding.results {
            let v = match src.as_str() {
                "exit_status" => {
                    let w = match results_ty.get(name) {
                        Some(Ty::Bv(w)) => *w,
                        _ => 64,
                    };
                    Value::bv(w, p.exit_status.ok_or("process did not exit")?)
                }
                "stdout" => Value::Bytes(p.stdout.clone()),
                "stderr" => Value::Bytes(p.stderr.clone()),
                other => return Err(format!("unknown process result source {other}")),
            };
            env.insert(format!("result.{name}"), v);
        }
        for f in &binding.files {
            let (exists, data) = p.files.get(&f.name).cloned().unwrap_or((false, Vec::new()));
            if let Some(st) = &f.observe_state {
                env.insert(format!("after.{st}"), Value::Bytes(data));
            }
            if let Some(st) = &f.exists_state {
                env.insert(format!("after.{st}"), Value::Bool(exists));
            }
        }
        return Ok(env);
    }
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
