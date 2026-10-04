//! Claims, aggregation, admission and diagnostics (docs/06).

use crate::emu::{self, Observation, Stop};
use crate::image::Monitor;
use crate::expr::{self, EvalCtx, Ty, Value};
use crate::plan::Case;
use crate::spec::{Binding, Contract, Isa};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Eval {
    SatisfiedInScope,
    Violated,
    Inconclusive,
    NotEvaluated,
}

#[derive(Clone, Debug)]
pub struct CaseClaim {
    pub property: String,
    pub eval: Eval,
    /// Why it is inconclusive / violated (short code).
    pub reason: Option<String>,
    /// Diagnostic detail for violations of semantic properties.
    pub detail: Option<serde_json::Value>,
}

pub fn machine_properties(binding: &Binding, monitors: &[Monitor]) -> Vec<String> {
    let mut v = if binding.target.is_process() {
        vec![
            "machine.exited".to_string(),
            "machine.memory.access".to_string(),
            "effects.no_forbidden".to_string(),
            "effects.output_within_limit".to_string(),
        ]
    } else {
        vec![
            "machine.returned".to_string(),
            "machine.abi.callee_saved".to_string(),
            "machine.memory.access".to_string(),
            "effects.no_forbidden".to_string(),
        ]
    };
    if !binding.target.is_process() && binding.target.isa == Isa::X86_64 {
        v.push("machine.abi.flags".to_string());
    }
    if binding.target.abi == "apple-arm64" {
        v.push("machine.abi.reserved".to_string());
    }
    for m in monitors {
        for k in ["requires", "ensures", "abi"] {
            v.push(format!("link.{}.{k}", m.symbol));
        }
    }
    v
}

/// Properties checked only in the cases that exercise them (boundary monitors).
pub fn is_conditional(property: &str) -> bool {
    property.starts_with("link.")
}

pub fn semantic_properties(contract: &Contract) -> Vec<String> {
    let mut v: Vec<String> = contract.ensures.iter().map(|e| format!("{}/{}", contract.id, e.id)).collect();
    for st in contract.state.keys() {
        if !contract.modifies.contains(st) {
            v.push(format!("{}/frame.{st}", contract.id));
        }
    }
    v
}

fn claim(property: &str, eval: Eval, reason: Option<&str>) -> CaseClaim {
    CaseClaim { property: property.to_string(), eval, reason: reason.map(str::to_string), detail: None }
}

/// Evaluate every required claim for one executed case.
pub fn judge_case(contract: &Contract, binding: &Binding, case: &Case, obs: &Observation) -> Vec<CaseClaim> {
    use Eval::*;
    let mut out = Vec::new();
    let process = binding.target.is_process();
    let returned = matches!(obs.stop, Stop::Returned | Stop::Exited { .. });
    let stopped_reason: Option<&str> = match &obs.stop {
        Stop::Returned | Stop::Exited { .. } => None,
        Stop::UnsupportedSyscall { .. } => Some("UNSUPPORTED_DURING_RUN"),
        Stop::OutputLimit { .. } => Some("OUTPUT_LIMIT"),
        Stop::BadReturn { .. } => Some("BAD_RETURN"),
        Stop::MemoryViolation { .. } => Some("STOPPED_BY_MEMORY_VIOLATION"),
        Stop::LeftCode { .. } => Some("LEFT_CODE_REGION"),
        Stop::InvalidInstruction { .. } => Some("UNSUPPORTED_DURING_RUN"),
        Stop::ForbiddenEffect { .. } => Some("STOPPED_BY_FORBIDDEN_EFFECT"),
        Stop::ReservedRegisterUsed { .. } => Some("STOPPED_BY_RESERVED_REGISTER"),
        Stop::BudgetExhausted { .. } => Some("BUDGET_EXHAUSTED"),
        Stop::Timeout { .. } => Some("TIMEOUT"),
        Stop::EngineError { .. } => Some("ENGINE_ERROR"),
        Stop::SetupError { .. } => Some("SETUP_ERROR"),
    };

    if process {
        out.push(match &obs.stop {
            Stop::Exited { .. } => claim("machine.exited", SatisfiedInScope, None),
            Stop::LeftCode { .. } | Stop::BadReturn { .. } => claim("machine.exited", Violated, stopped_reason),
            _ => claim("machine.exited", Inconclusive, stopped_reason),
        });
        out.push(match &obs.stop {
            Stop::OutputLimit { .. } => claim("effects.output_within_limit", Violated, Some("OUTPUT_LIMIT")),
            Stop::Exited { .. } => claim("effects.output_within_limit", SatisfiedInScope, None),
            _ => claim("effects.output_within_limit", Inconclusive, stopped_reason),
        });
    } else {
        // machine.returned
        out.push(match &obs.stop {
            Stop::Returned => claim("machine.returned", SatisfiedInScope, None),
            Stop::BadReturn { .. } | Stop::LeftCode { .. } => claim("machine.returned", Violated, stopped_reason),
            _ => claim("machine.returned", Inconclusive, stopped_reason),
        });
    }

    // ABI
    if process {
    } else if returned {
        let info = emu::isa_info(binding.target.isa);
        let changed: Vec<String> = info
            .callee_saved
            .iter()
            .filter(|r| obs.regs_in.get(**r) != obs.regs_out.get(**r))
            .map(|r| format!("{r}: 0x{:x} -> 0x{:x}", obs.regs_in[*r], obs.regs_out[*r]))
            .collect();
        let mut c = if changed.is_empty() {
            claim("machine.abi.callee_saved", SatisfiedInScope, None)
        } else {
            claim("machine.abi.callee_saved", Violated, Some("CALLEE_SAVED_CHANGED"))
        };
        if !changed.is_empty() {
            c.detail = Some(serde_json::json!({ "changed_registers": changed }));
        }
        out.push(c);
        if binding.target.isa == Isa::X86_64 {
            let df = obs.flags_out & (1 << 10) != 0;
            out.push(if df {
                claim("machine.abi.flags", Violated, Some("DF_SET_ON_RETURN"))
            } else {
                claim("machine.abi.flags", SatisfiedInScope, None)
            });
        }
    } else {
        out.push(claim("machine.abi.callee_saved", Inconclusive, stopped_reason));
        if binding.target.isa == Isa::X86_64 {
            out.push(claim("machine.abi.flags", Inconclusive, stopped_reason));
        }
    }
    if binding.target.abi == "apple-arm64" {
        out.push(match &obs.stop {
            Stop::ReservedRegisterUsed { .. } => claim("machine.abi.reserved", Violated, Some("X18_USED")),
            Stop::Returned | Stop::BadReturn { .. } => claim("machine.abi.reserved", SatisfiedInScope, None),
            _ => claim("machine.abi.reserved", Inconclusive, stopped_reason),
        });
    }

    // Memory monitor: complete only if execution reached the end.
    out.push(match &obs.stop {
        Stop::MemoryViolation { .. } => claim("machine.memory.access", Violated, Some("MEMORY_ACCESS_OUTSIDE_ALLOWED")),
        Stop::Returned | Stop::BadReturn { .. } | Stop::Exited { .. } => claim("machine.memory.access", SatisfiedInScope, None),
        _ => claim("machine.memory.access", Inconclusive, stopped_reason),
    });
    out.push(match &obs.stop {
        Stop::ForbiddenEffect { .. } => claim("effects.no_forbidden", Violated, Some("FORBIDDEN_EFFECT_ATTEMPTED")),
        Stop::Returned | Stop::BadReturn { .. } | Stop::Exited { .. } => claim("effects.no_forbidden", SatisfiedInScope, None),
        _ => claim("effects.no_forbidden", Inconclusive, stopped_reason),
    });

    // Boundary monitors: only for monitored callees that were actually called in this case.
    for (sym, m) in &obs.monitors {
        if m.calls == 0 {
            continue;
        }
        let pick = |kind: &str| m.details.iter().filter(|d| d["kind"] == kind).cloned().collect::<Vec<_>>();
        let mut c = if m.requires_violations > 0 {
            claim(&format!("link.{sym}.requires"), Violated, Some("CALLER_BROKE_CALLEE_REQUIRES"))
        } else {
            claim(&format!("link.{sym}.requires"), SatisfiedInScope, None)
        };
        if m.requires_violations > 0 {
            c.detail = Some(serde_json::json!({ "blame": "caller", "calls": m.calls, "violations": pick("requires") }));
        }
        out.push(c);
        let mut c = if m.ensures_violations > 0 {
            claim(&format!("link.{sym}.ensures"), Violated, Some("CALLEE_BROKE_ITS_ENSURES"))
        } else if m.inconclusive > 0 {
            claim(&format!("link.{sym}.ensures"), Inconclusive, Some("MONITOR_ERROR"))
        } else if m.returns == 0 {
            claim(&format!("link.{sym}.ensures"), Inconclusive, Some("CALLEE_DID_NOT_RETURN"))
        } else {
            claim(&format!("link.{sym}.ensures"), SatisfiedInScope, None)
        };
        if m.ensures_violations > 0 || m.inconclusive > 0 {
            c.detail = Some(serde_json::json!({ "blame": "callee", "calls": m.calls, "returns": m.returns, "violations": pick("ensures"), "errors": pick("monitor_error") }));
        }
        out.push(c);
        let mut c = if m.abi_violations > 0 {
            claim(&format!("link.{sym}.abi"), Violated, Some("CALLEE_SAVED_CHANGED_ACROSS_CALL"))
        } else if m.returns == 0 {
            claim(&format!("link.{sym}.abi"), Inconclusive, Some("CALLEE_DID_NOT_RETURN"))
        } else {
            claim(&format!("link.{sym}.abi"), SatisfiedInScope, None)
        };
        if m.abi_violations > 0 {
            c.detail = Some(serde_json::json!({ "blame": "callee", "violations": pick("abi") }));
        }
        out.push(c);
    }

    // Semantic claims need a proper return.
    let sem = semantic_properties(contract);
    if !returned {
        for p in sem {
            out.push(claim(&p, Inconclusive, Some(stopped_reason.unwrap_or("NOT_RETURNED"))));
        }
        return out;
    }
    let results_ty: BTreeMap<String, Ty> = contract.results.iter().map(|(k, v)| (k.clone(), v.ty)).collect();
    let state_ty: BTreeMap<String, Ty> = contract.state.iter().map(|(k, v)| (k.clone(), v.ty)).collect();
    let env = match emu::post_values(binding, case, obs, &results_ty, &state_ty) {
        Ok(e) => e,
        Err(m) => {
            for p in sem {
                let mut c = claim(&p, Inconclusive, Some("OBSERVATION_ERROR"));
                c.detail = Some(serde_json::json!({ "error": m }));
                out.push(c);
            }
            return out;
        }
    };
    let empty = BTreeMap::new();
    let cx = EvalCtx { vars: &env, region_addrs: &empty };
    for e in &contract.ensures {
        let p = format!("{}/{}", contract.id, e.id);
        match expr::eval(&e.expr, &cx) {
            Ok(Value::Bool(true)) => out.push(claim(&p, SatisfiedInScope, None)),
            Ok(_) => {
                let (_, trace) = expr::eval_traced(&e.expr, &cx, 16);
                let why = expr::explain_false(&e.expr, &cx);
                let mut c = claim(&p, Violated, Some("ENSURES_FALSE"));
                c.detail = Some(serde_json::json!({
                    "expr": e.src,
                    "why_false": why.into_iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(),
                    "subexpression_values": trace.into_iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(),
                }));
                out.push(c);
            }
            Err(m) => {
                let mut c = claim(&p, Inconclusive, Some("EVALUATION_ERROR"));
                c.detail = Some(serde_json::json!({ "expr": e.src, "error": m }));
                out.push(c);
            }
        }
    }
    for st in contract.state.keys() {
        if contract.modifies.contains(st) {
            continue;
        }
        let p = format!("{}/frame.{st}", contract.id);
        let b = env.get(&format!("before.{st}"));
        let a = env.get(&format!("after.{st}"));
        if b == a {
            out.push(claim(&p, SatisfiedInScope, None));
        } else {
            let mut c = claim(&p, Violated, Some("UNMODIFIABLE_STATE_CHANGED"));
            c.detail = Some(serde_json::json!({
                "before": b.map(|v| v.to_string()),
                "after": a.map(|v| v.to_string()),
            }));
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------- aggregation

#[derive(Debug, Serialize, Clone)]
pub struct ClaimSummary {
    pub property: String,
    pub evaluation: Eval,
    pub cases_satisfied: usize,
    pub cases_violated: usize,
    pub cases_inconclusive: usize,
    pub cases_not_evaluated: usize,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub inconclusive_reasons: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub counterexamples: Vec<String>,
}

pub fn aggregate(properties: &[String], per_case: &[(String, Vec<CaseClaim>)], skipped: usize) -> Vec<ClaimSummary> {
    let mut out = Vec::new();
    for p in properties {
        let mut s = ClaimSummary {
            property: p.clone(),
            evaluation: Eval::NotEvaluated,
            cases_satisfied: 0,
            cases_violated: 0,
            cases_inconclusive: 0,
            cases_not_evaluated: skipped,
            inconclusive_reasons: BTreeMap::new(),
            counterexamples: Vec::new(),
        };
        for (_, claims) in per_case {
            match claims.iter().find(|c| &c.property == p) {
                Some(c) => match c.eval {
                    Eval::SatisfiedInScope => s.cases_satisfied += 1,
                    Eval::Violated => s.cases_violated += 1,
                    Eval::Inconclusive => {
                        s.cases_inconclusive += 1;
                        *s.inconclusive_reasons.entry(c.reason.clone().unwrap_or_default()).or_default() += 1;
                    }
                    Eval::NotEvaluated => s.cases_not_evaluated += 1,
                },
                None if is_conditional(p) => {}
                None => s.cases_not_evaluated += 1,
            }
        }
        s.evaluation = if s.cases_violated > 0 {
            Eval::Violated
        } else if s.cases_inconclusive > 0 {
            Eval::Inconclusive
        } else if s.cases_not_evaluated > 0 || s.cases_satisfied == 0 {
            Eval::NotEvaluated
        } else {
            Eval::SatisfiedInScope
        };
        out.push(s);
    }
    out
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Admission {
    AcceptWithinScope,
    Hold,
    Reject,
}

/// docs/06 6.6. `valid_cases` counts cases that satisfied `requires`.
/// `generated` counts cases produced before `requires` filtering; `min_admitted` overrides the default floor.
pub fn admit(claims: &[ClaimSummary], valid_cases: usize, generated: usize, min_admitted: Option<u64>) -> (Admission, Vec<String>) {
    let mut reasons = Vec::new();
    if claims.iter().any(|c| c.evaluation == Eval::Violated) {
        for c in claims.iter().filter(|c| c.evaluation == Eval::Violated) {
            reasons.push(format!("VIOLATED: {}", c.property));
        }
        return (Admission::Reject, reasons);
    }
    if valid_cases == 0 {
        return (Admission::Hold, vec!["VACUOUS_SCOPE: no case satisfied `requires`".into()]);
    }
    // Near-vacuous scope: `requires` filtered out most generated cases (I5). The generator is not
    // producing the inputs the contract talks about; acceptance on the remainder would overstate the scope.
    let floor = min_admitted.unwrap_or_else(|| 100.min(generated.div_ceil(4) as u64)) as usize;
    if valid_cases < floor {
        reasons.push(format!(
            "LOW_ADMITTED_CASES: only {valid_cases} of {generated} generated cases satisfied `requires` (minimum {floor}); \
             make the generator produce valid inputs (e.g. a dependent `len`/`max`) or set limits.min_admitted_cases"
        ));
    }
    for c in claims {
        match c.evaluation {
            Eval::Inconclusive => {
                let why: Vec<String> = c.inconclusive_reasons.iter().map(|(k, n)| format!("{k}×{n}")).collect();
                reasons.push(format!("INCONCLUSIVE: {} ({})", c.property, why.join(", ")));
            }
            Eval::NotEvaluated => reasons.push(format!("NOT_EVALUATED: {}", c.property)),
            _ => {}
        }
    }
    if reasons.is_empty() { (Admission::AcceptWithinScope, reasons) } else { (Admission::Hold, reasons) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(p: &str, e: Eval) -> CaseClaim {
        CaseClaim { property: p.into(), eval: e, reason: Some("R".into()), detail: None }
    }

    // I1: inconclusive / unevaluated never becomes acceptance.
    #[test]
    fn i1_inconclusive_is_hold() {
        let props = vec!["a".to_string()];
        let per = vec![("c1".to_string(), vec![cc("a", Eval::SatisfiedInScope)]), ("c2".to_string(), vec![cc("a", Eval::Inconclusive)])];
        let s = aggregate(&props, &per, 0);
        assert_eq!(admit(&s, 2, 2, None).0, Admission::Hold);
    }

    #[test]
    fn i1_missing_claim_is_hold() {
        let props = vec!["a".to_string(), "b".to_string()];
        let per = vec![("c1".to_string(), vec![cc("a", Eval::SatisfiedInScope)])];
        let s = aggregate(&props, &per, 0);
        assert_eq!(admit(&s, 1, 1, None).0, Admission::Hold);
    }

    #[test]
    fn i1_skipped_cases_are_hold() {
        let props = vec!["a".to_string()];
        let per = vec![("c1".to_string(), vec![cc("a", Eval::SatisfiedInScope)])];
        let s = aggregate(&props, &per, 3);
        assert_eq!(admit(&s, 4, 4, None).0, Admission::Hold);
    }

    // I5: no valid case is HOLD even when nothing failed.
    #[test]
    fn i5_vacuous_is_hold() {
        let props = vec!["a".to_string()];
        let s = aggregate(&props, &[], 0);
        assert_eq!(admit(&s, 0, 0, None).0, Admission::Hold);
    }

    #[test]
    fn violation_survives_later_worker_failure() {
        let props = vec!["a".to_string()];
        let per = vec![("c1".to_string(), vec![cc("a", Eval::Violated)]), ("c2".to_string(), vec![cc("a", Eval::Inconclusive)])];
        let s = aggregate(&props, &per, 0);
        assert_eq!(admit(&s, 2, 2, None).0, Admission::Reject);
    }

    #[test]
    fn all_satisfied_is_accept() {
        let props = vec!["a".to_string()];
        let per = vec![("c1".to_string(), vec![cc("a", Eval::SatisfiedInScope)])];
        let s = aggregate(&props, &per, 0);
        assert_eq!(admit(&s, 1, 1, None).0, Admission::AcceptWithinScope);
    }
}
