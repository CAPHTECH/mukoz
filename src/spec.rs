//! Contract / Binding / Suite files (docs/04).

use crate::expr::{self, CheckCtx, Expr, Ty, TypeEnv};
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn sha256_hex(b: &[u8]) -> String {
    expr::hex(&Sha256::digest(b))
}

/// Read a TOML file, reject unknown fields via serde, and compute a digest of
/// its canonical JSON form (sorted keys).
fn load<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<(T, String)> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let raw: toml::Value = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let canon = serde_json::to_vec(&serde_json::to_value(&raw)?)?;
    let parsed: T = toml::from_str(&text).with_context(|| format!("validating {}", path.display()))?;
    Ok((parsed, sha256_hex(&canon)))
}

// ---------------------------------------------------------------- contract

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum VarDecl {
    Simple(String),
    Table(VarTable),
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct VarTable {
    #[serde(rename = "type")]
    pub ty: String,
    pub max_len: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarType {
    pub ty: Ty,
    pub max_len: u64,
}

impl VarDecl {
    fn resolve(&self, name: &str) -> Result<VarType> {
        let (t, max_len) = match self {
            VarDecl::Simple(s) => (s.as_str(), None),
            VarDecl::Table(t) => (t.ty.as_str(), t.max_len),
        };
        let ty = Ty::parse(t).ok_or_else(|| anyhow!("CONTRACT_TYPE_ERROR: `{name}` has unknown type `{t}`"))?;
        if ty == Ty::Bytes && max_len.is_none() {
            bail!("CONTRACT_GAP: bytes variable `{name}` needs max_len");
        }
        Ok(VarType { ty, max_len: max_len.unwrap_or(0) })
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ContractFile {
    schema: String,
    id: String,
    boundary: String,
    #[serde(default)]
    modifies: Vec<String>,
    #[serde(default)]
    inputs: BTreeMap<String, VarDecl>,
    #[serde(default)]
    state: BTreeMap<String, VarDecl>,
    #[serde(default)]
    results: BTreeMap<String, VarDecl>,
    #[serde(default)]
    requires: Vec<CondFile>,
    #[serde(default)]
    ensures: Vec<CondFile>,
    effects: EffectsFile,
    termination: TerminationFile,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct CondFile {
    id: Option<String>,
    expr: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct EffectsFile {
    allow: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct TerminationFile {
    kind: String,
}

#[derive(Debug, Clone)]
pub struct Cond {
    pub id: String,
    pub src: String,
    pub expr: Expr,
}

#[derive(Debug, Clone)]
pub struct Contract {
    pub id: String,
    pub digest: String,
    pub inputs: BTreeMap<String, VarType>,
    pub state: BTreeMap<String, VarType>,
    pub results: BTreeMap<String, VarType>,
    pub modifies: Vec<String>,
    pub requires: Vec<Cond>,
    pub ensures: Vec<Cond>,
}

impl Contract {
    pub fn load(path: &Path) -> Result<Contract> {
        let (f, digest): (ContractFile, _) = load(path)?;
        if f.schema != "mukoz.contract/1" {
            bail!("unsupported contract schema `{}`", f.schema);
        }
        if f.boundary != "routine" {
            bail!("UNSUPPORTED_FEATURE: boundary `{}` is not implemented yet (only `routine`)", f.boundary);
        }
        if f.termination.kind != "must_return" {
            bail!("UNSUPPORTED_FEATURE: termination `{}`", f.termination.kind);
        }
        if !f.effects.allow.is_empty() {
            bail!("UNSUPPORTED_FEATURE: routine effects other than none: {:?}", f.effects.allow);
        }
        let res = |m: &BTreeMap<String, VarDecl>| -> Result<BTreeMap<String, VarType>> {
            m.iter().map(|(k, v)| Ok((k.clone(), v.resolve(k)?))).collect()
        };
        let inputs = res(&f.inputs)?;
        let state = res(&f.state)?;
        let results = res(&f.results)?;
        for (k, v) in &results {
            if v.ty == Ty::Bytes {
                bail!("UNSUPPORTED_FEATURE: result `{k}` of type bytes");
            }
        }
        for m in &f.modifies {
            if !state.contains_key(m) {
                bail!("CONTRACT_GAP: modifies `{m}` is not a state variable");
            }
        }
        let mut pre_env = TypeEnv::new();
        for (k, v) in &inputs {
            pre_env.insert(format!("input.{k}"), v.ty);
        }
        for (k, v) in &state {
            pre_env.insert(format!("before.{k}"), v.ty);
        }
        let mut post_env = pre_env.clone();
        for (k, v) in &state {
            post_env.insert(format!("after.{k}"), v.ty);
        }
        for (k, v) in &results {
            post_env.insert(format!("result.{k}"), v.ty);
        }
        let conds = |list: Vec<CondFile>, env: &TypeEnv, kind: &str| -> Result<Vec<Cond>> {
            let mut out: Vec<Cond> = Vec::new();
            for (i, c) in list.into_iter().enumerate() {
                let id = c.id.unwrap_or_else(|| format!("{kind}{i}"));
                let e = expr::parse(&c.expr).map_err(|m| anyhow!("CONTRACT_TYPE_ERROR: {kind} `{id}`: {m}"))?;
                let t = expr::typecheck(&e, &CheckCtx { vars: env, allow_addr: false, regions: &[] })
                    .map_err(|m| anyhow!("CONTRACT_TYPE_ERROR: {kind} `{id}`: {m}"))?;
                if t != Ty::Bool {
                    bail!("CONTRACT_TYPE_ERROR: {kind} `{id}` must be bool, got {t}");
                }
                if out.iter().any(|o| o.id == id) {
                    bail!("CONTRACT_GAP: duplicate {kind} id `{id}`");
                }
                out.push(Cond { id, src: c.expr, expr: e });
            }
            Ok(out)
        };
        let requires = conds(f.requires, &pre_env, "requires")?;
        let ensures = conds(f.ensures, &post_env, "ensures")?;
        if ensures.is_empty() && results.is_empty() && f.modifies.is_empty() {
            bail!("CONTRACT_GAP: contract has no ensures, results or modified state");
        }
        Ok(Contract { id: f.id, digest, inputs, state, results, modifies: f.modifies, requires, ensures })
    }

    /// Types visible before execution (inputs and before-state).
    pub fn pre_env(&self) -> TypeEnv {
        let mut env = TypeEnv::new();
        for (k, v) in &self.inputs {
            env.insert(format!("input.{k}"), v.ty);
        }
        for (k, v) in &self.state {
            env.insert(format!("before.{k}"), v.ty);
        }
        env
    }
}

// ---------------------------------------------------------------- binding

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct BindingFile {
    schema: String,
    id: String,
    contract: String,
    target: String,
    entry: EntryFile,
    #[serde(default)]
    code_regions: Vec<CodeRegionFile>,
    #[serde(default)]
    arguments: BTreeMap<String, String>,
    #[serde(default)]
    results: BTreeMap<String, String>,
    #[serde(default)]
    stack: Option<StackFile>,
    #[serde(default)]
    regions: BTreeMap<String, RegionFile>,
    completion: CompletionFile,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct EntryFile {
    kind: String,
    offset: u64,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct CodeRegionFile {
    pub offset: u64,
    pub size: u64,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct StackFile {
    bytes: u64,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct RegionFile {
    size: String,
    init: Option<String>,
    access: String,
    observe_as: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct CompletionFile {
    kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Isa {
    X86_64,
    Aarch64,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub isa: Isa,
    pub abi: String,
    pub text: String,
}

impl Target {
    pub fn parse(s: &str) -> Result<Target> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 4 {
            bail!("target must be isa/format/abi/os, got `{s}`");
        }
        let isa = match parts[0] {
            "x86_64" => Isa::X86_64,
            "aarch64" => Isa::Aarch64,
            other => bail!("UNSUPPORTED_FEATURE: isa `{other}`"),
        };
        if parts[1] != "raw" || parts[3] != "none" {
            bail!("UNSUPPORTED_FEATURE: only raw routines (`<isa>/raw/<abi>/none`) are implemented, got `{s}`");
        }
        let ok = matches!((isa, parts[2]), (Isa::X86_64, "sysv-x86_64") | (Isa::Aarch64, "aapcs64") | (Isa::Aarch64, "apple-arm64"));
        if !ok {
            bail!("PLATFORM_COMBINATION_INVALID or unsupported ABI: `{s}`");
        }
        Ok(Target { isa, abi: parts[2].to_string(), text: s.to_string() })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    R,
    W,
    Rw,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub name: String,
    pub size: Expr,
    pub init: Option<Expr>,
    pub access: Access,
    /// `after.<state>` observed from this region at exit.
    pub observe_state: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub id: String,
    pub digest: String,
    pub target: Target,
    pub entry_offset: u64,
    pub code_regions: Vec<CodeRegionFile>,
    pub arguments: Vec<(String, Expr)>,
    pub results: Vec<(String, String)>,
    pub stack_bytes: u64,
    pub regions: Vec<Region>,
}

impl Binding {
    pub fn load(path: &Path, contract: &Contract, isa_regs: &dyn Fn(Isa, &str) -> bool) -> Result<Binding> {
        let (f, digest): (BindingFile, _) = load(path)?;
        if f.schema != "mukoz.binding/1" {
            bail!("unsupported binding schema `{}`", f.schema);
        }
        if f.contract != contract.id {
            bail!("BINDING_MISMATCH: binding is for contract `{}`, suite uses `{}`", f.contract, contract.id);
        }
        let target = Target::parse(&f.target)?;
        if f.entry.kind != "raw_offset" {
            bail!("UNSUPPORTED_FEATURE: entry kind `{}`", f.entry.kind);
        }
        if f.completion.kind != "return_to_sentinel" {
            bail!("UNSUPPORTED_FEATURE: completion `{}`", f.completion.kind);
        }
        let region_names: Vec<String> = f.regions.keys().cloned().collect();
        let pre = contract.pre_env();
        let cx = CheckCtx { vars: &pre, allow_addr: true, regions: &region_names };
        let mut arguments = Vec::new();
        for (reg, src) in &f.arguments {
            if !isa_regs(target.isa, reg) {
                bail!("BINDING_MISMATCH: `{reg}` is not an argument register of {}", target.text);
            }
            let e = expr::parse(src).map_err(|m| anyhow!("BINDING_MISMATCH: argument {reg}: {m}"))?;
            match expr::typecheck(&e, &cx).map_err(|m| anyhow!("BINDING_MISMATCH: argument {reg}: {m}"))? {
                Ty::Bv(_) | Ty::Bool => {}
                t => bail!("BINDING_MISMATCH: argument {reg} must be a bitvector, got {t}"),
            }
            arguments.push((reg.clone(), e));
        }
        let mut results = Vec::new();
        for (name, reg) in &f.results {
            if !contract.results.contains_key(name) {
                bail!("BINDING_MISMATCH: result `{name}` is not declared in the contract");
            }
            if !isa_regs(target.isa, reg) {
                bail!("BINDING_MISMATCH: `{reg}` is not a result register of {}", target.text);
            }
            results.push((name.clone(), reg.clone()));
        }
        for name in contract.results.keys() {
            if !f.results.contains_key(name) {
                bail!("BINDING_MISMATCH: contract result `{name}` has no binding");
            }
        }
        let mut regions = Vec::new();
        let mut observed = Vec::new();
        for (name, r) in &f.regions {
            let size = expr::parse(&r.size).map_err(|m| anyhow!("BINDING_MISMATCH: region {name} size: {m}"))?;
            let t = expr::typecheck(&size, &cx).map_err(|m| anyhow!("BINDING_MISMATCH: region {name} size: {m}"))?;
            if t != Ty::Bv(64) {
                bail!("BINDING_MISMATCH: region {name} size must be bv64, got {t}");
            }
            let init = match &r.init {
                Some(s) => {
                    let e = expr::parse(s).map_err(|m| anyhow!("BINDING_MISMATCH: region {name} init: {m}"))?;
                    expr::typecheck(&e, &cx).map_err(|m| anyhow!("BINDING_MISMATCH: region {name} init: {m}"))?;
                    Some(e)
                }
                None => None,
            };
            let access = match r.access.as_str() {
                "r" => Access::R,
                "w" => Access::W,
                "rw" => Access::Rw,
                a => bail!("BINDING_MISMATCH: region {name} access `{a}` (use r, w or rw)"),
            };
            let observe_state = match &r.observe_as {
                Some(p) => {
                    let st = p.strip_prefix("after.").ok_or_else(|| anyhow!("BINDING_MISMATCH: observe_as must be `after.<state>`"))?;
                    if !contract.state.contains_key(st) {
                        bail!("BINDING_MISMATCH: observe_as `{p}` is not a contract state variable");
                    }
                    observed.push(st.to_string());
                    Some(st.to_string())
                }
                None => None,
            };
            regions.push(Region { name: name.clone(), size, init, access, observe_state });
        }
        for st in contract.state.keys() {
            if !observed.contains(st) {
                bail!("BINDING_MISMATCH: contract state `{st}` is not observed by any region (observe_as)");
            }
        }
        Ok(Binding {
            id: f.id,
            digest,
            target,
            entry_offset: f.entry.offset,
            code_regions: f.code_regions,
            arguments,
            results,
            stack_bytes: f.stack.map(|s| s.bytes).unwrap_or(16384),
            regions,
        })
    }
}

// ---------------------------------------------------------------- suite

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct SuiteFile {
    schema: String,
    id: String,
    contract: String,
    binding: String,
    #[serde(default = "default_executors")]
    executors: Vec<String>,
    artifact: Option<ArtifactFile>,
    #[serde(default)]
    generate: GenerateFile,
    #[serde(default)]
    limits: LimitsFile,
    #[serde(default)]
    regressions: RegressionsFile,
}

fn default_executors() -> Vec<String> {
    vec!["emulated".into()]
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ArtifactFile {
    path: String,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct GenerateFile {
    seed: Option<String>,
    boundary: Option<String>,
    random_cases: Option<u64>,
    #[serde(default)]
    vars: BTreeMap<String, VarGenFile>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct VarGenFile {
    /// Length expression for bytes variables (may refer to earlier variables).
    pub len: Option<String>,
    /// Explicit extra values (bv: hex/decimal strings; bytes: hex strings).
    #[serde(default)]
    pub values: Vec<String>,
    /// Byte alphabet for bytes variables, e.g. "nonzero".
    pub bytes: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct LimitsFile {
    instructions_per_case: Option<u64>,
    wall_ms_per_case: Option<u64>,
    max_cases: Option<u64>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct RegressionsFile {
    include: bool,
}

impl Default for RegressionsFile {
    fn default() -> Self {
        RegressionsFile { include: true }
    }
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub instructions_per_case: u64,
    pub wall_ms_per_case: u64,
    pub max_cases: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Suite {
    pub id: String,
    pub digest: String,
    pub dir: PathBuf,
    pub contract_path: PathBuf,
    pub binding_path: PathBuf,
    pub artifact_path: Option<PathBuf>,
    pub seed: u64,
    pub boundary_product: bool,
    pub random_cases: u64,
    pub vars: BTreeMap<String, VarGenFile>,
    pub limits: Limits,
    pub include_regressions: bool,
}

pub const MAX_CASES_HARD: u64 = 8192;

impl Suite {
    pub fn load(path: &Path) -> Result<Suite> {
        let (f, digest): (SuiteFile, _) = load(path)?;
        if f.schema != "mukoz.suite/1" {
            bail!("unsupported suite schema `{}`", f.schema);
        }
        if f.executors != ["emulated"] {
            bail!("REQUIRED_CAPABILITY_UNAVAILABLE: executors {:?} (only `emulated` is implemented)", f.executors);
        }
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let seed_text = f.generate.seed.unwrap_or_else(|| "0".into());
        let seed = seed_text.parse::<u64>().unwrap_or_else(|_| {
            let h = Sha256::digest(seed_text.as_bytes());
            u64::from_le_bytes(h[..8].try_into().unwrap())
        });
        let boundary_product = match f.generate.boundary.as_deref() {
            None | Some("product") => true,
            Some("none") => false,
            Some(o) => bail!("generate.boundary must be `product` or `none`, got `{o}`"),
        };
        let max_cases = f.limits.max_cases.unwrap_or(MAX_CASES_HARD);
        if max_cases > MAX_CASES_HARD {
            bail!("limits.max_cases {max_cases} exceeds the hard limit {MAX_CASES_HARD}");
        }
        Ok(Suite {
            id: f.id,
            digest,
            contract_path: dir.join(&f.contract),
            binding_path: dir.join(&f.binding),
            artifact_path: f.artifact.map(|a| dir.join(a.path)),
            dir,
            seed,
            boundary_product,
            random_cases: f.generate.random_cases.unwrap_or(1024),
            vars: f.generate.vars,
            limits: Limits {
                instructions_per_case: f.limits.instructions_per_case.unwrap_or(100_000),
                wall_ms_per_case: f.limits.wall_ms_per_case.unwrap_or(1000),
                max_cases,
            },
            include_regressions: f.regressions.include,
        })
    }
}
