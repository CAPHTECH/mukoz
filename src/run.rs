//! `check` and `replay`: plan → execute → judge → assess (docs/07).

use crate::emu::{self, Executor, Observation};
use crate::image::{self, Image, LinkOptions};
use crate::expr::{self, Value};
use crate::judge::{self, Admission, CaseClaim, Eval};
use crate::plan::{self, Case};
use crate::spec::{Binding, Contract, Entry, Format, Suite, sha256_hex};
use crate::store::Store;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::json;
use std::path::{Path, PathBuf};

pub const EVALUATOR_VERSION: &str = concat!("mukoz ", env!("CARGO_PKG_VERSION"));
const MAX_ARTIFACT: u64 = 64 << 20;
const INLINE_FINDINGS: usize = 3;
const CX_PER_PROPERTY: usize = 3;
const NATIVE_TIMEOUT_LIMIT: usize = 3;

pub struct Loaded {
    pub suite: Suite,
    pub contract: Contract,
    pub binding: Binding,
    /// The file under test: the artifact, or the entry module's file when a link file is used.
    pub artifact_path: PathBuf,
    pub artifact: Vec<u8>,
    /// Digest of everything executed (all modules when linked).
    pub artifact_digest: String,
    pub image: Image,
    pub modules: Vec<(String, PathBuf)>,
}

pub fn load(suite_path: &Path, artifact: Option<&Path>, module_overrides: &[(String, PathBuf)]) -> Result<Loaded> {
    let suite = Suite::load(suite_path)?;
    let contract = Contract::load(&suite.contract_path)?;
    let binding = Binding::load(&suite.binding_path, &contract, &emu::is_arg_or_result_reg)?;
    let read_artifact = |p: &Path| -> Result<Vec<u8>> {
        let meta = std::fs::metadata(p).with_context(|| format!("artifact {}", p.display()))?;
        if meta.len() > MAX_ARTIFACT {
            bail!("artifact is {} bytes; limit is {MAX_ARTIFACT}", meta.len());
        }
        Ok(std::fs::read(p)?)
    };
    let given = || -> Result<PathBuf> {
        match artifact {
            Some(p) => Ok(p.to_path_buf()),
            None => suite.artifact_path.clone().ok_or_else(|| anyhow!("USAGE: no artifact: set [artifact] path in the suite or pass --artifact")),
        }
    };
    if !module_overrides.is_empty() && binding.link.is_none() {
        bail!("USAGE: --module needs a binding with a link file");
    }
    let stack_lo = emu::STACK_TOP - binding.stack_bytes.div_ceil(0x1000) * 0x1000;
    let (image, artifact_path, bytes, digest, modules) = match (&binding.entry, &binding.link) {
        (Entry::Symbol(sym), Some(link)) => {
            let opts = LinkOptions { entry_override: artifact, module_overrides };
            let (img, used) = image::link(link, sym, binding.target.isa, &opts, &emu::is_arg_or_result_reg)?;
            let entry_mod = sym.split_once('.').map(|(m, _)| m).unwrap_or_default();
            let path = used.iter().find(|(n, _)| n == entry_mod).map(|(_, p)| p.clone()).unwrap_or_default();
            let bytes = std::fs::read(&path).unwrap_or_default();
            let digest = sha256_hex(img.parts.iter().map(|(n, d)| format!("{n}={d}\n")).collect::<String>().as_bytes());
            (img, path, bytes, digest, used)
        }
        (Entry::ElfEntry, None) if binding.target.format == Format::Elf => {
            let p = given()?;
            let b = read_artifact(&p)?;
            let img = Image::elf(&b, binding.target.isa, stack_lo)?;
            let d = sha256_hex(&b);
            (img, p, b, d, Vec::new())
        }
        (Entry::Offset(o), None) => {
            let p = given()?;
            let b = read_artifact(&p)?;
            let regions: Vec<(u64, u64)> = binding.code_regions.iter().map(|r| (r.offset, r.size)).collect();
            let img = Image::raw(&b, *o, &regions)?;
            let d = sha256_hex(&b);
            (img, p, b, d, Vec::new())
        }
        _ => bail!("BINDING_MISMATCH: entry and link do not fit together"),
    };
    Ok(Loaded { suite, contract, binding, artifact_path, artifact: bytes, artifact_digest: digest, image, modules })
}

fn subject_context(l: &Loaded) -> String {
    let s = format!(
        "artifact={}\ncontract={}\nbinding={}\nsuite={}\ngenerator={}\nengine={}\nevaluator={}\n",
        l.artifact_digest,
        l.contract.digest,
        l.binding.digest,
        l.suite.digest,
        plan::GENERATOR_VERSION,
        emu::ENGINE,
        EVALUATOR_VERSION
    );
    sha256_hex(s.as_bytes())
}

fn values_json(case: &Case) -> serde_json::Value {
    serde_json::Value::Object(case.values.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
}

fn observed_json(l: &Loaded, case: &Case, obs: &Observation) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    if let Some(p) = &obs.process {
        let show = |b: &[u8]| json!({ "text": String::from_utf8_lossy(b), "hex": expr::hex(b), "len": b.len() });
        m.insert("exit_status".into(), json!(p.exit_status));
        m.insert("stdout".into(), show(&p.stdout));
        m.insert("stderr".into(), show(&p.stderr));
        for (name, (exists, data)) in &p.files {
            m.insert(format!("file.{name}"), json!({ "exists": exists, "text": String::from_utf8_lossy(data), "hex": expr::hex(data), "len": data.len() }));
        }
        m.insert("syscalls_executed".into(), json!(p.syscall_count));
        m.insert("last_syscalls".into(), json!(p.syscalls));
        let _ = case;
        return serde_json::Value::Object(m);
    }
    for (name, reg) in &l.binding.results {
        if let Some(raw) = obs.regs_out.get(reg) {
            let w = match l.contract.results.get(name).map(|v| v.ty) {
                Some(expr::Ty::Bv(w)) => w,
                _ => 64,
            };
            m.insert(format!("result.{name}"), Value::bv(w, *raw).to_json());
            m.insert(format!("register.{reg}"), json!(format!("0x{raw:016x}")));
        }
    }
    for r in &l.binding.regions {
        if let (Some(st), Some(bytes)) = (&r.observe_state, obs.regions_out.get(&r.name)) {
            m.insert(format!("after.{st}"), json!({ "hex": expr::hex(bytes), "len": bytes.len() }));
        }
    }
    let _ = case;
    serde_json::Value::Object(m)
}

/// How the executors of one run were set up (docs/03 3.5, docs/08 8.4 rule 3-4).
pub struct Platform {
    pub executors: Vec<String>,
    pub native: Option<String>,
    /// Ok(permit) or Err(reason code and detail) for the native executor.
    pub native_state: Option<Result<crate::policy::Permit, String>>,
    pub applied_isolation: Vec<&'static str>,
    pub probe_host: Option<String>,
    pub policy: Option<(String, String)>,
}

impl Platform {
    fn native_ok(&self) -> bool {
        matches!(self.native_state, Some(Ok(_)))
    }
    fn json(&self) -> serde_json::Value {
        let mut v = json!({
            "executors": self.executors,
            "host": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        });
        if self.executors.iter().any(|e| e == "emulated") {
            v["emulated"] = json!({ "engine": emu::ENGINE, "process_isolation": "in-process (worker separation not implemented yet)" });
        }
        if let Some(n) = &self.native {
            v[n.as_str()] = match &self.native_state {
                Some(Ok(p)) => json!({
                    "permitted_by": p.basis,
                    "isolation_required_by_policy": p.isolation_required,
                    "isolation_applied": self.applied_isolation,
                    "probe_host_id": self.probe_host,
                }),
                Some(Err(e)) => json!({ "not_run": e }),
                None => json!(null),
            };
        }
        if let Some((path, digest)) = &self.policy {
            v["policy"] = json!({ "path": path, "digest": digest });
        }
        v
    }
}

fn executed_files(l: &Loaded) -> Vec<PathBuf> {
    if l.modules.is_empty() { vec![l.artifact_path.clone()] } else { l.modules.iter().map(|(_, p)| p.clone()).collect() }
}

fn platform_for(l: &Loaded, store: &Store, policy_path: Option<&Path>) -> Result<Platform> {
    let executors = l.suite.executors.clone();
    let native = executors.iter().find(|e| e.starts_with("native-")).cloned();
    let mut p = Platform { executors, native: native.clone(), native_state: None, applied_isolation: Vec::new(), probe_host: None, policy: None };
    let Some(n) = native else { return Ok(p) };
    let policy = crate::policy::Policy::find(policy_path, &store.root)?;
    p.policy = policy.as_ref().map(|x| (x.path.display().to_string(), x.digest.clone()));
    let capable = match n.as_str() {
        "native-routine" => crate::native::host_can_execute(&l.binding),
        _ => Err("REQUIRED_CAPABILITY_UNAVAILABLE: native-process is not implemented yet".to_string()),
    };
    p.native_state = Some(match capable {
        Err(e) => Err(e),
        Ok(()) => {
            let probe = crate::host::current_probe(store);
            p.probe_host = probe["host_id"].as_str().map(str::to_string);
            let mut r = crate::policy::permit(policy.as_ref(), &n, &l.binding.target.text, &executed_files(l), &l.artifact_digest, &probe);
            if n == "native-routine" && r.is_ok() && !probe["capabilities"]["seccomp_strict"]["confirmed"].as_bool().unwrap_or(false) {
                r = Err("NATIVE_NOT_PERMITTED: seccomp was not confirmed on this host (required for native-routine, docs/08 8.4)".into());
            }
            r
        }
    });
    Ok(p)
}

fn reason_code(e: &str) -> String {
    e.split(':').next().unwrap_or(e).to_string()
}

/// Claims from a native-routine observation: page-granular memory protection cannot establish
/// the byte-granular memory claim (docs/02 2.3), so a completed run leaves it NOT_EVALUATED.
fn native_routine_claims(l: &Loaded, case: &Case, obs: &Observation) -> Vec<CaseClaim> {
    let mut v = judge::judge_case(&l.contract, &l.binding, case, obs);
    for c in v.iter_mut() {
        if c.property == "machine.memory.access" && c.eval == Eval::SatisfiedInScope {
            c.eval = Eval::NotEvaluated;
            c.reason = Some("REQUIRED_CAPABILITY_UNAVAILABLE".into());
            c.detail = Some(json!({ "why": "native-routine detects out-of-bounds accesses only at page granularity" }));
        }
    }
    v
}

fn stop_class(s: &emu::Stop) -> &'static str {
    use emu::Stop::*;
    match s {
        Returned => "returned",
        BadReturn { .. } => "bad_return",
        Exited { .. } => "exited",
        MemoryViolation { .. } | LeftCode { .. } => "fault",
        InvalidInstruction { .. } => "invalid_instruction",
        ForbiddenEffect { .. } => "forbidden_effect",
        UnsupportedSyscall { .. } => "unsupported",
        OutputLimit { .. } => "output_limit",
        ReservedRegisterUsed { .. } => "reserved_register",
        BudgetExhausted { .. } | Timeout { .. } => "nonterminating",
        EngineError { .. } => "engine_error",
        SetupError { .. } => "setup_error",
    }
}

pub fn differential_property(native: &str) -> String {
    format!("differential.emulated_vs_{native}")
}

/// Compare one case across executors (docs/06 6.8). A difference whose cause is unknown is
/// INCONCLUSIVE with BACKEND_DIVERGENCE (it is not blamed on the subject); a difference that only
/// reflects what the native executor cannot observe is NOT_EVALUATED for that case.
fn compare(l: &Loaded, native: &str, e: &Observation, n: &Observation) -> CaseClaim {
    let prop = differential_property(native);
    let mk = |eval: Eval, reason: Option<&str>, detail: Option<serde_json::Value>| CaseClaim { property: prop.clone(), eval, reason: reason.map(str::to_string), detail };
    let (ce, cn) = (stop_class(&e.stop), stop_class(&n.stop));
    let both = || json!({ "emulated": { "stop": e.stop }, native: { "stop": n.stop } });
    if matches!(cn, "setup_error" | "engine_error") {
        return mk(Eval::Inconclusive, Some("NATIVE_EXECUTION_ERROR"), Some(both()));
    }
    if matches!(ce, "setup_error" | "engine_error") {
        return mk(Eval::Inconclusive, Some("EMULATED_EXECUTION_ERROR"), Some(both()));
    }
    if ce != cn && (ce == "nonterminating" || cn == "nonterminating") {
        return mk(Eval::NotEvaluated, Some("BUDGETS_DIFFER"), Some(both()));
    }
    if ce == "fault" && cn == "returned" {
        return mk(Eval::NotEvaluated, Some("BELOW_NATIVE_PAGE_GRANULARITY"), Some(both()));
    }
    if ce != cn {
        return mk(Eval::Inconclusive, Some("BACKEND_DIVERGENCE"), Some(both()));
    }
    if ce != "returned" {
        return mk(Eval::SatisfiedInScope, None, None);
    }
    let info = emu::isa_info(l.binding.target.isa);
    let mut diffs = Vec::new();
    let mut regs: Vec<String> = l.binding.results.iter().map(|(_, r)| r.clone()).collect();
    regs.extend(info.callee_saved.iter().map(|r| r.to_string()));
    regs.sort();
    regs.dedup();
    for r in &regs {
        let (a, b) = (e.regs_out.get(r), n.regs_out.get(r));
        if a != b {
            diffs.push(json!({ "register": r, "emulated": a.map(|x| format!("0x{x:016x}")), native: b.map(|x| format!("0x{x:016x}")) }));
        }
    }
    if l.binding.target.isa == crate::spec::Isa::X86_64 && (e.flags_out ^ n.flags_out) & (1 << 10) != 0 {
        diffs.push(json!({ "flag": "DF", "emulated": e.flags_out >> 10 & 1, native: n.flags_out >> 10 & 1 }));
    }
    for (name, a) in &e.regions_out {
        let b = n.regions_out.get(name);
        if Some(a) != b {
            diffs.push(json!({ "region": name, "emulated": expr::hex(a), native: b.map(|x| expr::hex(x)) }));
        }
    }
    if diffs.is_empty() { mk(Eval::SatisfiedInScope, None, None) } else { mk(Eval::Inconclusive, Some("BACKEND_DIVERGENCE"), Some(json!({ "differences": diffs }))) }
}

pub struct CheckOpts<'a> {
    pub policy: Option<&'a Path>,
    pub suite: &'a Path,
    pub artifact: Option<&'a Path>,
    pub modules: &'a [(String, PathBuf)],
    pub fail_fast: bool,
    pub store: &'a Store,
}

pub fn check(o: &CheckOpts) -> Result<(serde_json::Value, Admission)> {
    let l = load(o.suite, o.artifact, o.modules)?;
    let target = l.binding.target.text.clone();
    let (regs, reg_na) = if l.suite.include_regressions { o.store.load_regressions(&l.contract, &target)? } else { (Vec::new(), 0) };
    let reg_ids: Vec<String> = regs.iter().map(|(id, _)| id.clone()).collect();
    let generated = plan::generate(&l.contract, &l.suite, regs.into_iter().map(|(_, c)| c).collect(), reg_na)?;
    o.store.put_object(&l.artifact)?;
    let context = subject_context(&l);
    let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let run_id = format!("run-{}", &sha256_hex(format!("{context}{started}").as_bytes())[..12]);

    let exec = Executor {
        image: &l.image,
        contract: &l.contract,
        binding: &l.binding,
        insn_limit: l.suite.limits.instructions_per_case,
        timeout_ms: l.suite.limits.wall_ms_per_case,
        vary_placement: l.suite.vary_placement,
    };
    let mut properties = judge::machine_properties(&l.binding, &l.image.monitors);
    properties.extend(judge::semantic_properties(&l.contract));
    let mut platform = platform_for(&l, o.store, o.policy)?;
    let emulated = platform.executors.iter().any(|e| e == "emulated");
    let differential = emulated && platform.native.is_some();
    if differential {
        properties.push(differential_property(platform.native.as_deref().unwrap_or_default()));
    }
    let native_wall = match &platform.native_state {
        Some(Ok(p)) => p.max_wall_ms_per_case.map_or(l.suite.limits.wall_ms_per_case, |z| z.min(l.suite.limits.wall_ms_per_case)),
        _ => l.suite.limits.wall_ms_per_case,
    };

    let mut per_case: Vec<(String, Vec<CaseClaim>)> = Vec::new();
    let mut observations: Vec<Observation> = Vec::new();
    let mut divergent: Vec<(usize, Observation)> = Vec::new();
    let mut native_timeouts = 0usize;
    let mut skipped = 0;
    let total = generated.cases.len();
    for (i, case) in generated.cases.iter().enumerate() {
        let obs_e = if emulated { Some(exec.run(case)) } else { None };
        // A native case that does not finish costs the whole wall limit; after a few, the rest are
        // not run (NOT_EVALUATED, recorded) instead of stalling the check.
        let emulated_nonterminating = obs_e.as_ref().is_some_and(|e| stop_class(&e.stop) == "nonterminating");
        let native_skip: Option<&str> = if !platform.native_ok() {
            None
        } else if native_timeouts >= NATIVE_TIMEOUT_LIMIT {
            Some("NATIVE_TIMEOUT_LIMIT")
        } else if emulated_nonterminating {
            Some("EMULATED_NONTERMINATING")
        } else {
            None
        };
        let obs_n = if platform.native_ok() && native_skip.is_none() {
            let r = crate::native::run(&l.image, &l.binding, case, l.suite.vary_placement, native_wall);
            platform.applied_isolation = r.applied_isolation;
            if matches!(r.obs.stop, emu::Stop::Timeout { .. }) {
                native_timeouts += 1;
            }
            Some(r.obs)
        } else {
            None
        };
        let mut claims = match (&obs_e, &obs_n) {
            (Some(e), _) => judge::judge_case(&l.contract, &l.binding, case, e),
            (None, Some(n)) => native_routine_claims(&l, case, n),
            (None, None) => {
                let why = native_skip.map(str::to_string).or_else(|| platform.native_state.as_ref().and_then(|r| r.as_ref().err()).cloned()).unwrap_or_default();
                properties
                    .iter()
                    .filter(|p| !judge::is_conditional(p))
                    .map(|p| CaseClaim { property: p.clone(), eval: Eval::NotEvaluated, reason: Some(reason_code(&why)), detail: Some(json!({ "why": why })) })
                    .collect()
            }
        };
        if differential {
            let native = platform.native.clone().unwrap_or_default();
            claims.push(match (&obs_e, &obs_n) {
                (Some(e), Some(n)) => {
                    let c = compare(&l, &native, e, n);
                    if c.eval == Eval::Inconclusive && divergent.len() < CX_PER_PROPERTY {
                        divergent.push((i, n.clone()));
                    }
                    c
                }
                _ => {
                    let why = native_skip.map(str::to_string).or_else(|| platform.native_state.as_ref().and_then(|r| r.as_ref().err()).cloned()).unwrap_or_default();
                    CaseClaim { property: differential_property(&native), eval: Eval::NotEvaluated, reason: Some(reason_code(&why)), detail: Some(json!({ "why": why })) }
                }
            });
        }
        let failed = claims.iter().any(|c| c.eval == Eval::Violated);
        per_case.push((case.id.clone(), claims));
        let obs = match (obs_e, obs_n) {
            (Some(e), _) => e,
            (None, Some(n)) => n,
            (None, None) => emu::Observation {
                stop: emu::Stop::SetupError { error: "not executed".into() },
                regs_in: Default::default(),
                regs_out: Default::default(),
                flags_out: 0,
                regions_out: Default::default(),
                recent: Vec::new(),
                instructions: 0,
                process: None,
                monitors: Default::default(),
            },
        };
        observations.push(obs);
        if failed && o.fail_fast {
            skipped = total - i - 1;
            break;
        }
    }
    let summaries = judge::aggregate(&properties, &per_case, skipped);
    let generated_total = total + generated.stats.excluded_by_requires;
    let (admission, reasons) = judge::admit(&summaries, total, generated_total, l.suite.limits.min_admitted_cases);

    // Counterexamples: up to CX_PER_PROPERTY per violated property.
    let mut summaries = summaries;
    let mut findings = Vec::new();
    let mut new_regressions = 0;
    for s in summaries.iter_mut().filter(|s| s.evaluation == Eval::Violated) {
        for (idx, (case_id, claims)) in per_case.iter().enumerate() {
            if s.counterexamples.len() >= CX_PER_PROPERTY {
                break;
            }
            let Some(c) = claims.iter().find(|c| c.property == s.property && c.eval == Eval::Violated) else { continue };
            let case = &generated.cases[idx];
            let obs = &observations[idx];
            let cx_id = format!("cx-{}", &sha256_hex(format!("{run_id}{case_id}{}", s.property).as_bytes())[..12]);
            let finding = json!({
                "counterexample_id": cx_id,
                "property": s.property,
                "case_id": case_id,
                "case_origin": case.origin,
                "inputs": values_json(case),
                "stop": obs.stop,
                "reason": c.reason,
                "detail": c.detail,
                "observed": if matches!(obs.stop, emu::Stop::Returned | emu::Stop::Exited { .. }) || obs.process.is_some() { observed_json(&l, case, obs) } else { json!(null) },
                "monitors": obs.monitors,
                "instructions_executed": obs.instructions,
                "recent_instructions": obs.recent.iter().rev().take(16).rev().collect::<Vec<_>>(),
            });
            let item = json!({
                "kind": "counterexample",
                "id": cx_id,
                "run_id": run_id,
                "subject_context": context,
                "suite": std::fs::canonicalize(o.suite)?.display().to_string(),
                "artifact": { "path": std::fs::canonicalize(&l.artifact_path)?.display().to_string(), "digest": l.artifact_digest },
                "modules": l.modules.iter().map(|(n, p)| json!({ "name": n, "path": std::fs::canonicalize(p).map(|p| p.display().to_string()).unwrap_or_default() })).collect::<Vec<_>>(),
                "contract": { "id": l.contract.id, "digest": l.contract.digest },
                "binding": { "id": l.binding.id, "digest": l.binding.digest },
                "property": s.property,
                "failure_predicate": c.reason,
                "case": { "id": case_id, "origin": case.origin, "values": values_json(case), "filler_seed": format!("0x{:016x}", case.filler_seed) },
                "registers_in": obs.regs_in.iter().map(|(k, v)| (k.clone(), json!(format!("0x{v:016x}")))).collect::<serde_json::Map<_, _>>(),
                "registers_out": obs.regs_out.iter().map(|(k, v)| (k.clone(), json!(format!("0x{v:016x}")))).collect::<serde_json::Map<_, _>>(),
                "stop": obs.stop,
                "detail": c.detail,
                "recent_instructions": obs.recent,
                "execution_platform": platform.json(),
                "executor": if emulated { "emulated" } else { platform.native.as_deref().unwrap_or_default() },
            });
            o.store.put_item(&cx_id, &item)?;
            if o.store.add_regression(&l.contract, &target, case, &cx_id)? {
                new_regressions += 1;
            }
            s.counterexamples.push(cx_id);
            if findings.len() < INLINE_FINDINGS && !findings.iter().any(|f: &serde_json::Value| f["property"] == json!(s.property)) {
                findings.push(finding);
            }
        }
    }

    // Divergences: evidence for the differential claim (inspect with `show`).
    let mut divergences = Vec::new();
    for (idx, n) in &divergent {
        let case = &generated.cases[*idx];
        let e = &observations[*idx];
        let native = platform.native.clone().unwrap_or_default();
        let c = per_case[*idx].1.iter().find(|c| c.property == differential_property(&native)).cloned();
        let dv_id = format!("dv-{}", &sha256_hex(format!("{run_id}{}", case.id).as_bytes())[..12]);
        let side = |o: &Observation| json!({
            "stop": o.stop,
            "registers_out": o.regs_out.iter().map(|(k, v)| (k.clone(), json!(format!("0x{v:016x}")))).collect::<serde_json::Map<_, _>>(),
            "flags_out": format!("0x{:x}", o.flags_out),
            "regions_out": o.regions_out.iter().map(|(k, v)| (k.clone(), json!(expr::hex(v)))).collect::<serde_json::Map<_, _>>(),
        });
        let item = json!({
            "kind": "divergence",
            "id": dv_id,
            "run_id": run_id,
            "subject_context": context,
            "property": differential_property(&native),
            "reason": c.as_ref().and_then(|c| c.reason.clone()),
            "detail": c.as_ref().and_then(|c| c.detail.clone()),
            "case": { "id": case.id, "origin": case.origin, "values": values_json(case), "filler_seed": format!("0x{:016x}", case.filler_seed) },
            "registers_in": e.regs_in.iter().map(|(k, v)| (k.clone(), json!(format!("0x{v:016x}")))).collect::<serde_json::Map<_, _>>(),
            "emulated": side(e),
            native.as_str(): side(n),
            "recent_instructions_emulated": e.recent,
            "note": "the cause (subject, binding, ABI, engine or CPU) is not determined; this is not a counterexample against the contract",
        });
        o.store.put_item(&dv_id, &item)?;
        divergences.push(dv_id);
    }
    if let Some(s) = summaries.iter_mut().find(|s| s.property.starts_with("differential.")) {
        s.counterexamples = divergences.clone();
    }

    let completed = per_case.len();
    let failed_cases = per_case.iter().filter(|(_, c)| c.iter().any(|x| x.eval == Eval::Violated)).count();
    let mut limitations = vec!["enumerated_cases_not_exhaustive".to_string()];
    if emulated {
        limitations.push("engine_runs_in_process".into());
    }
    match (&platform.native, platform.native_ok()) {
        (None, _) => limitations.push("emulated_only_not_native_execution".into()),
        (Some(n), false) => limitations.push(format!("{n}_not_run")),
        (Some(n), true) if n == "native-routine" => limitations.push("native_routine_memory_checks_page_granular_only".into()),
        _ => {}
    }
    if native_timeouts >= NATIVE_TIMEOUT_LIMIT {
        limitations.push(format!("native_cases_stopped_after_{NATIVE_TIMEOUT_LIMIT}_timeouts"));
    }
    if !l.suite.include_regressions {
        limitations.push("regression_cases_excluded".into());
    }
    if skipped > 0 {
        limitations.push(format!("fail_fast_skipped_{skipped}_cases"));
    }
    if let Some(n) = generated.stats.boundary_product_exceeded_at {
        limitations.push(format!(
            "boundary_product_reduced_to_one_variable_at_a_time: product reached {n} cases, over max_cases/2 = {}",
            (l.suite.limits.max_cases / 2).max(1)
        ));
    }
    if generated.stats.excluded_by_requires > 0 {
        limitations.push(format!("requires_excluded_{}_of_{generated_total}_generated_cases", generated.stats.excluded_by_requires));
    }
    let data = json!({
        "run_id": run_id,
        "execution": "completed",
        "assessment": {
            "admission": admission,
            "reasons": reasons,
            "context_match": true,
            "release_authorized": false,
            "subject_context": context,
            "scope": {
                "contract": l.contract.id,
                "binding": l.binding.id,
                "target": target,
                "artifact": { "path": l.artifact_path.display().to_string(), "digest": l.artifact_digest, "bytes": l.artifact.len() },
                "modules": l.image.parts.iter().map(|(n, d)| {
                    let m = l.image.modules.iter().find(|m| &m.name == n);
                    json!({ "name": n, "digest": d, "base": m.map(|m| format!("0x{:x}", m.base)), "bytes": m.map(|m| m.size) })
                }).collect::<Vec<_>>(),
                "monitors": l.image.monitors.iter().map(|m| json!({ "symbol": m.symbol, "contract": m.contract.id })).collect::<Vec<_>>(),
                "platform": platform.json(),
                "quantification": "enumerated_cases_not_exhaustive",
                "generator": plan::GENERATOR_VERSION,
                "cases_planned": total,
                "cases_completed": completed,
                "cases_failed": failed_cases,
                "cases_skipped": skipped,
                "plan_stats": generated.stats,
                "input_summary": plan::input_summary(&generated.cases),
                "regression_cases_run": reg_ids.len().min(completed),
                "new_regression_cases": new_regressions,
            },
            "limitations": limitations,
        },
        "claims": summaries,
        "findings": findings,
    });
    o.store.put_item(&run_id, &json!({ "kind": "run", "data": data }))?;
    Ok((data, admission))
}

pub fn replay(store: &Store, cx_id: &str, artifact: Option<&Path>) -> Result<serde_json::Value> {
    let item = store.get_item(cx_id)?;
    if item["kind"] != "counterexample" {
        bail!("`{cx_id}` is not a counterexample");
    }
    let suite_path = PathBuf::from(item["suite"].as_str().unwrap_or_default());
    let art = match artifact {
        Some(a) => a.to_path_buf(),
        None => PathBuf::from(item["artifact"]["path"].as_str().unwrap_or_default()),
    };
    let modules: Vec<(String, PathBuf)> = item["modules"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| Some((m["name"].as_str()?.to_string(), PathBuf::from(m["path"].as_str()?)))).collect())
        .unwrap_or_default();
    // With a link file, keep the recorded modules except the one --artifact replaces.
    let l = load(&suite_path, Some(&art), &modules.iter().filter(|_| artifact.is_none()).cloned().collect::<Vec<_>>())?;
    let mut values = expr::ValEnv::new();
    let vals = item["case"]["values"].as_object().ok_or_else(|| anyhow!("counterexample has no values"))?;
    for (prefix, m) in [("input", &l.contract.inputs), ("before", &l.contract.state)] {
        for (name, vt) in m {
            let key = format!("{prefix}.{name}");
            let v = vals.get(&key).and_then(|j| Value::from_json(vt.ty, j)).ok_or_else(|| anyhow!("counterexample value `{key}` does not fit the current contract"))?;
            values.insert(key, v);
        }
    }
    let seed = item["case"]["filler_seed"].as_str().and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok()).unwrap_or(0);
    let case = Case { id: item["case"]["id"].as_str().unwrap_or("replay").to_string(), origin: plan::Origin::Regression, values, filler_seed: seed };
    let exec = Executor { image: &l.image, contract: &l.contract, binding: &l.binding, insn_limit: l.suite.limits.instructions_per_case, timeout_ms: l.suite.limits.wall_ms_per_case, vary_placement: l.suite.vary_placement };
    let obs = exec.run(&case);
    let claims = judge::judge_case(&l.contract, &l.binding, &case, &obs);
    let property = item["property"].as_str().unwrap_or_default();
    let target_claim = claims.iter().find(|c| c.property == property);
    let same_subject = item["artifact"]["digest"] == json!(l.artifact_digest) && item["contract"]["digest"] == json!(l.contract.digest) && item["binding"]["digest"] == json!(l.binding.digest);
    Ok(json!({
        "counterexample_id": cx_id,
        "property": property,
        "same_artifact_contract_binding": same_subject,
        "artifact": { "path": l.artifact_path.display().to_string(), "digest": l.artifact_digest },
        "property_now": target_claim.map(|c| c.eval),
        "meaning": if same_subject { "replay on the original subject" } else { "regression check on a changed subject; passing means only that this counterexample no longer reproduces" },
        "stop": obs.stop,
        "observed": if matches!(obs.stop, emu::Stop::Returned | emu::Stop::Exited { .. }) || obs.process.is_some() { observed_json(&l, &case, &obs) } else { json!(null) },
        "monitors": obs.monitors,
        "claims": claims.iter().map(|c| json!({ "property": c.property, "evaluation": c.eval, "reason": c.reason, "detail": c.detail })).collect::<Vec<_>>(),
        "recent_instructions": obs.recent,
    }))
}
