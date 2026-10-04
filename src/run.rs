//! `check` and `replay`: plan → execute → judge → assess (docs/07).

use crate::emu::{self, Executor, Observation};
use crate::expr::{self, Value};
use crate::judge::{self, Admission, CaseClaim, Eval};
use crate::plan::{self, Case};
use crate::spec::{Binding, Contract, Suite, sha256_hex};
use crate::store::Store;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::json;
use std::path::{Path, PathBuf};

pub const EVALUATOR_VERSION: &str = concat!("mukoz ", env!("CARGO_PKG_VERSION"));
const MAX_ARTIFACT: u64 = 64 << 20;
const INLINE_FINDINGS: usize = 3;
const CX_PER_PROPERTY: usize = 3;

pub struct Loaded {
    pub suite: Suite,
    pub contract: Contract,
    pub binding: Binding,
    pub artifact_path: PathBuf,
    pub artifact: Vec<u8>,
    pub artifact_digest: String,
}

pub fn load(suite_path: &Path, artifact: Option<&Path>) -> Result<Loaded> {
    let suite = Suite::load(suite_path)?;
    let contract = Contract::load(&suite.contract_path)?;
    let binding = Binding::load(&suite.binding_path, &contract, &emu::is_arg_or_result_reg)?;
    let artifact_path = match artifact {
        Some(p) => p.to_path_buf(),
        None => suite.artifact_path.clone().ok_or_else(|| anyhow!("USAGE: no artifact: set [artifact] path in the suite or pass --artifact"))?,
    };
    let meta = std::fs::metadata(&artifact_path).with_context(|| format!("artifact {}", artifact_path.display()))?;
    if meta.len() > MAX_ARTIFACT {
        bail!("artifact is {} bytes; limit is {MAX_ARTIFACT}", meta.len());
    }
    let artifact = std::fs::read(&artifact_path)?;
    let artifact_digest = sha256_hex(&artifact);
    Ok(Loaded { suite, contract, binding, artifact_path, artifact, artifact_digest })
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

fn platform_json() -> serde_json::Value {
    json!({
        "executor": "emulated",
        "host": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        "engine": emu::ENGINE,
        "process_isolation": "in-process (worker separation not implemented yet)",
    })
}

pub struct CheckOpts<'a> {
    pub suite: &'a Path,
    pub artifact: Option<&'a Path>,
    pub fail_fast: bool,
    pub store: &'a Store,
}

pub fn check(o: &CheckOpts) -> Result<(serde_json::Value, Admission)> {
    let l = load(o.suite, o.artifact)?;
    let target = l.binding.target.text.clone();
    let (regs, reg_na) = if l.suite.include_regressions { o.store.load_regressions(&l.contract, &target)? } else { (Vec::new(), 0) };
    let reg_ids: Vec<String> = regs.iter().map(|(id, _)| id.clone()).collect();
    let generated = plan::generate(&l.contract, &l.suite, regs.into_iter().map(|(_, c)| c).collect(), reg_na)?;
    o.store.put_object(&l.artifact)?;
    let context = subject_context(&l);
    let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let run_id = format!("run-{}", &sha256_hex(format!("{context}{started}").as_bytes())[..12]);

    let exec = Executor {
        code: &l.artifact,
        binding: &l.binding,
        insn_limit: l.suite.limits.instructions_per_case,
        timeout_ms: l.suite.limits.wall_ms_per_case,
    };
    let mut properties = judge::machine_properties(&l.binding);
    properties.extend(judge::semantic_properties(&l.contract));

    let mut per_case: Vec<(String, Vec<CaseClaim>)> = Vec::new();
    let mut observations: Vec<Observation> = Vec::new();
    let mut skipped = 0;
    let total = generated.cases.len();
    for (i, case) in generated.cases.iter().enumerate() {
        let obs = exec.run(case);
        let claims = judge::judge_case(&l.contract, &l.binding, case, &obs);
        let failed = claims.iter().any(|c| c.eval == Eval::Violated);
        per_case.push((case.id.clone(), claims));
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
                "observed": if matches!(obs.stop, emu::Stop::Returned) { observed_json(&l, case, obs) } else { json!(null) },
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
                "execution_platform": platform_json(),
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

    let completed = per_case.len();
    let failed_cases = per_case.iter().filter(|(_, c)| c.iter().any(|x| x.eval == Eval::Violated)).count();
    let mut limitations = vec![
        "enumerated_cases_not_exhaustive".to_string(),
        "emulated_only_not_native_execution".to_string(),
        "engine_runs_in_process".to_string(),
    ];
    if !l.suite.include_regressions {
        limitations.push("regression_cases_excluded".into());
    }
    if skipped > 0 {
        limitations.push(format!("fail_fast_skipped_{skipped}_cases"));
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
                "platform": platform_json(),
                "quantification": "enumerated_cases_not_exhaustive",
                "generator": plan::GENERATOR_VERSION,
                "cases_planned": total,
                "cases_completed": completed,
                "cases_failed": failed_cases,
                "cases_skipped": skipped,
                "plan_stats": generated.stats,
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
    let l = load(&suite_path, Some(&art))?;
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
    let exec = Executor { code: &l.artifact, binding: &l.binding, insn_limit: l.suite.limits.instructions_per_case, timeout_ms: l.suite.limits.wall_ms_per_case };
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
        "observed": if matches!(obs.stop, emu::Stop::Returned) { observed_json(&l, &case, &obs) } else { json!(null) },
        "claims": claims.iter().map(|c| json!({ "property": c.property, "evaluation": c.eval, "reason": c.reason, "detail": c.detail })).collect::<Vec<_>>(),
        "recent_instructions": obs.recent,
    }))
}
