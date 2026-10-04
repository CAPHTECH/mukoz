//! What gets mapped before execution (docs/13): a raw file, a static ELF, or
//! several raw modules joined by a link file, plus boundary monitors.

use crate::expr::{Expr, Ty};
use crate::spec::{Binding, Contract, Isa, Suite};
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CODE_BASE: u64 = 0x0010_0000;
/// Module `i` of a link file is mapped at CODE_BASE + i * MODULE_STRIDE.
pub const MODULE_STRIDE: u64 = 0x0010_0000;
pub const MAX_MODULES: usize = 16;
/// Import table: 8-byte absolute addresses, read-only, one page.
pub const IMPORT_TABLE: u64 = 0x000f_0000;
pub const IMPORT_SLOTS: u64 = 512;
/// Writable zero-filled data area of a raw process.
pub const DATA_BASE: u64 = 0x1000_0000;
/// Lowest address of the area reserved for regions, stack and sentinel.
const RESERVED_LO: u64 = 0x0dea_0000;

#[derive(Debug, Clone)]
pub struct Segment {
    pub addr: u64,
    /// Bytes at `addr`; the rest up to `mem_size` is zero.
    pub data: Vec<u8>,
    pub mem_size: u64,
    pub read: bool,
    pub write: bool,
    pub exec: bool,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct Module {
    /// Empty for a single unnamed module (offsets are then printed as plain hex).
    pub name: String,
    pub base: u64,
    pub size: u64,
}

/// How a monitored callee's contract values are recovered from the call.
#[derive(Debug, Clone)]
pub enum ArgSrc {
    /// `input.x` / `before.x` bitvector or bool.
    Value(String, Ty),
    /// `len(input.b)`: length of a bytes variable.
    Len(String),
    /// `addr(region)`.
    Addr(String),
}

#[derive(Debug, Clone)]
pub struct RegionPlan {
    pub name: String,
    /// Contract variable initialized from this region (`input.b` / `before.b`).
    pub var: Option<String>,
    /// Size from the length of `var` (known from a `len(...)` argument), or an expression.
    pub size_from_len: Option<String>,
    pub size_expr: Option<Expr>,
    pub observe: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Monitor {
    pub symbol: String,
    pub addr: u64,
    pub contract: Contract,
    pub binding: Binding,
    pub args: Vec<(String, ArgSrc)>,
    pub regions: Vec<RegionPlan>,
}

#[derive(Debug, Clone)]
pub struct Image {
    pub segments: Vec<Segment>,
    pub entry: u64,
    pub code_ranges: Vec<(u64, u64)>,
    pub modules: Vec<Module>,
    pub monitors: Vec<Monitor>,
    /// Digests of every file that went into the image (module name, sha256).
    pub parts: Vec<(String, String)>,
}

impl Image {
    /// Label an address for diagnostics: `0x1c` (single raw module), `store+0x1c`, or `abs:0x...`.
    pub fn locate(&self, addr: u64) -> String {
        for m in &self.modules {
            // An unnamed raw module labels nearby addresses by offset too (as 0.1 did); an ELF (base 0) uses absolute addresses.
            let span = match (m.name.is_empty(), m.base) {
                (true, 0) => u64::MAX,
                (true, _) => 0x1000_0000,
                (false, _) => MODULE_STRIDE,
            };
            if addr >= m.base && addr - m.base < span {
                return if m.name.is_empty() { format!("0x{:x}", addr - m.base) } else { format!("{}+0x{:x}", m.name, addr - m.base) };
            }
        }
        format!("abs:0x{addr:x}")
    }

    /// Bytes at an executable address (for the recent-instruction listing).
    pub fn code_bytes(&self, addr: u64, n: usize) -> Option<&[u8]> {
        self.segments.iter().find(|s| s.exec && addr >= s.addr && addr + n as u64 <= s.addr + s.data.len() as u64).map(|s| {
            let o = (addr - s.addr) as usize;
            &s.data[o..o + n]
        })
    }

    pub fn raw(code: &[u8], entry_offset: u64, code_regions: &[(u64, u64)]) -> Result<Image> {
        let len = code.len() as u64;
        if len == 0 {
            bail!("SETUP: artifact is empty");
        }
        if entry_offset >= len {
            bail!("SETUP: entry offset {entry_offset} is outside the artifact ({len} bytes)");
        }
        for (o, n) in code_regions {
            if o.checked_add(*n).is_none_or(|e| e > len) {
                bail!("BINDING_MISMATCH: code region {o}+{n} exceeds the artifact ({len} bytes)");
            }
        }
        let code_ranges = if code_regions.is_empty() {
            vec![(CODE_BASE, CODE_BASE + len)]
        } else {
            code_regions.iter().map(|(o, n)| (CODE_BASE + o, CODE_BASE + o + n)).collect()
        };
        Ok(Image {
            segments: vec![Segment { addr: CODE_BASE, data: code.to_vec(), mem_size: len, read: true, write: false, exec: true, label: "code (read-only)".into() }],
            entry: CODE_BASE + entry_offset,
            code_ranges,
            modules: vec![Module { name: String::new(), base: CODE_BASE, size: len }],
            monitors: Vec::new(),
            parts: Vec::new(),
        })
    }

    /// Static, non-PIE ELF64 little-endian executable.
    pub fn elf(bytes: &[u8], isa: Isa, stack_lo: u64) -> Result<Image> {
        let u16_at = |o: usize| -> Result<u64> { Ok(u16::from_le_bytes(bytes.get(o..o + 2).ok_or_else(|| anyhow!("ELF: truncated"))?.try_into()?) as u64) };
        let u32_at = |o: usize| -> Result<u64> { Ok(u32::from_le_bytes(bytes.get(o..o + 4).ok_or_else(|| anyhow!("ELF: truncated"))?.try_into()?) as u64) };
        let u64_at = |o: usize| -> Result<u64> { Ok(u64::from_le_bytes(bytes.get(o..o + 8).ok_or_else(|| anyhow!("ELF: truncated"))?.try_into()?)) };
        if bytes.get(..4) != Some(b"\x7fELF") {
            bail!("FORMAT_MISMATCH: not an ELF file");
        }
        if bytes[4] != 2 || bytes[5] != 1 {
            bail!("UNSUPPORTED_FEATURE: only 64-bit little-endian ELF");
        }
        let machine = u16_at(18)?;
        let want = match isa {
            Isa::X86_64 => 62,
            Isa::Aarch64 => 183,
        };
        if machine != want {
            bail!("FORMAT_MISMATCH: ELF e_machine {machine} does not match the target ISA");
        }
        match u16_at(16)? {
            2 => {}
            3 => bail!("UNSUPPORTED_FEATURE: position-independent (ET_DYN) executables; link with -no-pie / -static"),
            t => bail!("UNSUPPORTED_FEATURE: ELF type {t} (only ET_EXEC)"),
        }
        let entry = u64_at(24)?;
        let phoff = u64_at(32)? as usize;
        let phentsize = u16_at(54)? as usize;
        let phnum = u16_at(56)? as usize;
        let mut segments = Vec::new();
        let mut code_ranges = Vec::new();
        for i in 0..phnum {
            let o = phoff + i * phentsize;
            let ty = u32_at(o)?;
            match ty {
                1 => {}
                2 | 3 => bail!("UNRESOLVED_DEPENDENCY: dynamic linking (PT_INTERP / PT_DYNAMIC) is not modeled; build a static executable"),
                7 => bail!("UNSUPPORTED_FEATURE: thread-local storage (PT_TLS)"),
                _ => continue,
            }
            let flags = u32_at(o + 4)?;
            let off = u64_at(o + 8)? as usize;
            let vaddr = u64_at(o + 16)?;
            let filesz = u64_at(o + 32)? as usize;
            let memsz = u64_at(o + 40)?;
            if memsz == 0 {
                continue;
            }
            let data = bytes.get(off..off + filesz).ok_or_else(|| anyhow!("ELF: segment {i} lies outside the file"))?.to_vec();
            let end = vaddr.checked_add(memsz).ok_or_else(|| anyhow!("ELF: segment {i} wraps"))?;
            if vaddr < 0x1_0000 || end > RESERVED_LO.min(stack_lo) || (vaddr < IMPORT_TABLE + 0x1000 && end > IMPORT_TABLE) {
                bail!("UNSUPPORTED_FEATURE: ELF segment {i} at [0x{vaddr:x}, 0x{end:x}) overlaps memory Mukoz reserves (below 0x10000, the import table, or 0x{RESERVED_LO:x} and up)");
            }
            if memsz > 64 << 20 {
                bail!("UNSUPPORTED_FEATURE: ELF segment {i} is larger than 64 MiB");
            }
            let (r, w, x) = (flags & 4 != 0, flags & 2 != 0, flags & 1 != 0);
            if x {
                code_ranges.push((vaddr, vaddr + filesz as u64));
            }
            let label = format!("ELF segment {i} ({}{}{})", if r { "r" } else { "-" }, if w { "w" } else { "-" }, if x { "x" } else { "-" });
            segments.push(Segment { addr: vaddr, data, mem_size: memsz, read: r, write: w, exec: x, label });
        }
        if code_ranges.is_empty() {
            bail!("FORMAT_MISMATCH: ELF has no executable segment");
        }
        if !code_ranges.iter().any(|(lo, hi)| entry >= *lo && entry < *hi) {
            bail!("FORMAT_MISMATCH: ELF entry 0x{entry:x} is not in an executable segment");
        }
        Ok(Image { segments, entry, code_ranges, modules: vec![Module { name: String::new(), base: 0, size: u64::MAX >> 1 }], monitors: Vec::new(), parts: Vec::new() })
    }
}

// ---------------------------------------------------------------- link files

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct LinkFile {
    schema: String,
    modules: Vec<ModuleFile>,
    #[serde(default)]
    imports: BTreeMap<String, String>,
    #[serde(default)]
    monitors: BTreeMap<String, String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ModuleFile {
    name: String,
    path: String,
    #[serde(default)]
    exports: BTreeMap<String, u64>,
}

pub struct LinkOptions<'a> {
    /// Replace the file of the module that holds the entry symbol (`--artifact`).
    pub entry_override: Option<&'a Path>,
    /// Replace any module's file (`--module name=path`).
    pub module_overrides: &'a [(String, PathBuf)],
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !s.starts_with(|c: char| c.is_ascii_digit())
}

/// Load a link file: map modules, fill the import table, prepare monitors.
/// Returns the image and the path of every module file actually used.
pub fn link(path: &Path, entry_symbol: &str, isa: Isa, opts: &LinkOptions, isa_regs: &dyn Fn(Isa, &str) -> bool) -> Result<(Image, Vec<(String, PathBuf)>)> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading link file {}", path.display()))?;
    let f: LinkFile = toml::from_str(&text).with_context(|| format!("validating link file {}", path.display()))?;
    if f.schema != "mukoz.link/1" {
        bail!("unsupported link schema `{}`", f.schema);
    }
    if f.modules.is_empty() || f.modules.len() > MAX_MODULES {
        bail!("LINK_ERROR: a link file needs 1..={MAX_MODULES} modules");
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let (entry_mod, entry_sym) = entry_symbol.split_once('.').ok_or_else(|| anyhow!("LINK_ERROR: entry symbol `{entry_symbol}` must be `module.symbol`"))?;
    let mut segments = Vec::new();
    let mut modules = Vec::new();
    let mut code_ranges = Vec::new();
    let mut symbols: BTreeMap<String, u64> = BTreeMap::new();
    let mut used = Vec::new();
    let mut parts = Vec::new();
    for (i, m) in f.modules.iter().enumerate() {
        if !valid_name(&m.name) || modules.iter().any(|x: &Module| x.name == m.name) {
            bail!("LINK_ERROR: module name `{}` is invalid or repeated (letters, digits, _)", m.name);
        }
        let file = if let Some((_, p)) = opts.module_overrides.iter().find(|(n, _)| n == &m.name) {
            p.clone()
        } else if m.name == entry_mod && opts.entry_override.is_some() {
            opts.entry_override.unwrap().to_path_buf()
        } else {
            dir.join(&m.path)
        };
        let code = std::fs::read(&file).with_context(|| format!("module `{}`: {}", m.name, file.display()))?;
        if code.is_empty() || code.len() as u64 > MODULE_STRIDE {
            bail!("LINK_ERROR: module `{}` is {} bytes (1..={} allowed)", m.name, code.len(), MODULE_STRIDE);
        }
        let base = CODE_BASE + i as u64 * MODULE_STRIDE;
        for (sym, off) in &m.exports {
            if !valid_name(sym) {
                bail!("LINK_ERROR: export `{sym}` of `{}` is not a valid name", m.name);
            }
            if *off >= code.len() as u64 {
                bail!("LINK_ERROR: export `{}.{sym}` at offset {off} is outside the module ({} bytes)", m.name, code.len());
            }
            symbols.insert(format!("{}.{sym}", m.name), base + off);
        }
        parts.push((m.name.clone(), crate::spec::sha256_hex(&code)));
        code_ranges.push((base, base + code.len() as u64));
        modules.push(Module { name: m.name.clone(), base, size: code.len() as u64 });
        segments.push(Segment { addr: base, mem_size: code.len() as u64, data: code, read: true, write: false, exec: true, label: format!("module `{}` (read-only)", m.name) });
        used.push((m.name.clone(), file));
    }
    let mut table = vec![0u8; (IMPORT_SLOTS * 8) as usize];
    let mut max_slot = 0;
    for (slot, sym) in &f.imports {
        let k: u64 = slot.parse().map_err(|_| anyhow!("LINK_ERROR: import slot `{slot}` is not a number"))?;
        if k >= IMPORT_SLOTS {
            bail!("LINK_ERROR: import slot {k} is beyond {}", IMPORT_SLOTS - 1);
        }
        let a = *symbols.get(sym).ok_or_else(|| anyhow!("LINK_ERROR: import slot {k} names `{sym}`, which no module exports (known: {:?})", symbols.keys().collect::<Vec<_>>()))?;
        table[(k * 8) as usize..(k * 8 + 8) as usize].copy_from_slice(&a.to_le_bytes());
        max_slot = max_slot.max(k + 1);
    }
    if !f.imports.is_empty() {
        segments.push(Segment { addr: IMPORT_TABLE, mem_size: IMPORT_SLOTS * 8, data: table, read: true, write: false, exec: false, label: "import table (read-only)".into() });
    }
    let entry = *symbols.get(entry_symbol).ok_or_else(|| anyhow!("LINK_ERROR: entry symbol `{entry_symbol}` is not exported (module `{entry_mod}`, symbol `{entry_sym}`)"))?;
    let mut monitors = Vec::new();
    for (sym, suite_path) in &f.monitors {
        let addr = *symbols.get(sym).ok_or_else(|| anyhow!("LINK_ERROR: monitor for `{sym}`, which no module exports"))?;
        let suite = Suite::load(&dir.join(suite_path)).with_context(|| format!("monitor `{sym}`"))?;
        let contract = Contract::load(&suite.contract_path).with_context(|| format!("monitor `{sym}`"))?;
        let binding = Binding::load(&suite.binding_path, &contract, isa_regs).with_context(|| format!("monitor `{sym}`"))?;
        if binding.target.isa != isa || binding.target.is_process() {
            bail!("LINK_ERROR: monitor `{sym}` must use a routine binding for the same ISA");
        }
        monitors.push(plan_monitor(sym, addr, contract, binding)?);
    }
    let _ = max_slot;
    Ok((Image { segments, entry, code_ranges, modules, monitors, parts }, used))
}

/// Work out how to recover the callee's contract values from registers and memory at the call.
fn plan_monitor(sym: &str, addr: u64, contract: Contract, binding: Binding) -> Result<Monitor> {
    let ty_of = |key: &str| -> Option<Ty> {
        let (p, n) = key.split_once('.')?;
        match p {
            "input" => contract.inputs.get(n).map(|v| v.ty),
            "before" => contract.state.get(n).map(|v| v.ty),
            _ => None,
        }
    };
    let unsupported = |what: String| anyhow!("MONITOR_UNSUPPORTED: `{sym}`: {what}; monitors recover values only from `input.x`, `len(input.b)` and `addr(region)` arguments");
    let mut args = Vec::new();
    let mut known: Vec<String> = Vec::new();
    let mut lens: Vec<String> = Vec::new();
    for (reg, e) in &binding.arguments {
        let src = match e {
            Expr::Path(p) if p.len() == 2 => {
                let key = p.join(".");
                match ty_of(&key) {
                    Some(t @ (Ty::Bv(_) | Ty::Bool)) => {
                        known.push(key.clone());
                        ArgSrc::Value(key, t)
                    }
                    _ => return Err(unsupported(format!("argument {reg} = {key}"))),
                }
            }
            Expr::Call(f, a) if f == "len" && a.len() == 1 => match &a[0] {
                Expr::Path(p) if p.len() == 2 && ty_of(&p.join(".")) == Some(Ty::Bytes) => {
                    lens.push(p.join("."));
                    ArgSrc::Len(p.join("."))
                }
                _ => return Err(unsupported(format!("argument {reg}"))),
            },
            Expr::Call(f, a) if f == "addr" && a.len() == 1 => match &a[0] {
                Expr::Path(p) if p.len() == 1 => ArgSrc::Addr(p[0].clone()),
                _ => return Err(unsupported(format!("argument {reg}"))),
            },
            other => return Err(unsupported(format!("argument {reg} = {other}"))),
        };
        args.push((reg.clone(), src));
    }
    let mut regions = Vec::new();
    for r in &binding.regions {
        if !args.iter().any(|(_, a)| matches!(a, ArgSrc::Addr(n) if n == &r.name)) {
            return Err(unsupported(format!("region `{}` is not passed as addr(...)", r.name)));
        }
        let var = match &r.init {
            None => None,
            Some(Expr::Path(p)) if p.len() == 2 && ty_of(&p.join(".")) == Some(Ty::Bytes) => Some(p.join(".")),
            Some(e) => return Err(unsupported(format!("region `{}` init = {e}", r.name))),
        };
        let size_from_len = match &r.size {
            Expr::Call(f, a) if f == "len" && a.len() == 1 => match &a[0] {
                Expr::Path(p) if lens.contains(&p.join(".")) => Some(p.join(".")),
                _ => None,
            },
            _ => None,
        };
        if size_from_len.is_none() && r.monitor_size.is_none() {
            return Err(unsupported(format!(
                "the size of region `{}` cannot be recovered from the arguments; add `monitor_size = \"<expr over the arguments>\"` to the region",
                r.name
            )));
        }
        if let Some(v) = &var {
            known.push(v.clone());
        }
        regions.push(RegionPlan { name: r.name.clone(), var, size_from_len, size_expr: if r.monitor_size.is_some() { r.monitor_size.clone() } else { None }, observe: r.observe_state.clone() });
    }
    // Regions sized by a length argument first, so monitor_size expressions can use their contents.
    regions.sort_by_key(|r| r.size_from_len.is_none());
    for k in contract.inputs.keys().map(|k| format!("input.{k}")).chain(contract.state.keys().map(|k| format!("before.{k}"))) {
        if !known.contains(&k) {
            return Err(unsupported(format!("`{k}` is not recovered from any argument or region")));
        }
    }
    Ok(Monitor { symbol: sym.to_string(), addr, contract, binding, args, regions })
}
