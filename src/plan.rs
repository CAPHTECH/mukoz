//! Case generation (docs/04 4.7, 4.8).

use crate::expr::{self, EvalCtx, Ty, ValEnv, Value, mask};
use crate::spec::{Contract, Suite, VarGenFile, VarType};
use anyhow::{Result, anyhow, bail};
use std::collections::BTreeMap;

/// Generator version; recorded in evidence. Bump when generated values change.
pub const GENERATOR_VERSION: &str = "mukoz-gen/1";

pub struct SplitMix64(u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Regression,
    Boundary,
    Random,
}

#[derive(Clone, Debug)]
pub struct Case {
    pub id: String,
    pub origin: Origin,
    /// `input.*` and `before.*` values.
    pub values: ValEnv,
    /// Seed for registers the binding does not set.
    pub filler_seed: u64,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct PlanStats {
    pub regression_cases: usize,
    pub regression_not_applicable: usize,
    pub boundary_cases: usize,
    pub random_cases: usize,
    pub excluded_by_requires: usize,
    pub requires_eval_errors: usize,
    pub duplicate_inputs: usize,
}

#[derive(Clone)]
struct VarSlot {
    key: String,
    name: String,
    vt: VarType,
    generator: VarGenFile,
}

fn slots(contract: &Contract, suite: &Suite) -> Result<Vec<VarSlot>> {
    let mut out = Vec::new();
    for (prefix, m) in [("input", &contract.inputs), ("before", &contract.state)] {
        for (name, vt) in m {
            out.push(VarSlot {
                key: format!("{prefix}.{name}"),
                name: name.clone(),
                vt: *vt,
                generator: suite.vars.get(name).cloned().unwrap_or_default(),
            });
        }
    }
    for k in suite.vars.keys() {
        if !out.iter().any(|s| &s.name == k) {
            bail!("generate.vars.{k}: no such input or state variable");
        }
    }
    // Fields that do not apply to a variable's type are errors, never silently ignored.
    for s in &out {
        let g = &s.generator;
        let bad = |field: &str, ty: &str| anyhow!("generate.vars.{}.{field} does not apply to a {ty} variable", s.name);
        match s.vt.ty {
            Ty::Bool => {
                if g.len.is_some() { return Err(bad("len", "bool")); }
                if g.max.is_some() { return Err(bad("max", "bool")); }
                if g.bytes.is_some() { return Err(bad("bytes", "bool")); }
                if !g.values.is_empty() { return Err(bad("values", "bool")); }
            }
            Ty::Bv(_) => {
                if g.len.is_some() { return Err(bad("len", "bitvector")); }
                if g.bytes.is_some() { return Err(bad("bytes", "bitvector")); }
            }
            Ty::Bytes => {
                if g.max.is_some() {
                    return Err(anyhow!("generate.vars.{}.max does not apply to a bytes variable (use `len`, or max_len in the contract)", s.name));
                }
                if let Some(a) = &g.bytes {
                    if a != "nonzero" && a != "ascii" {
                        bail!("generate.vars.{}.bytes must be `nonzero` or `ascii`, got `{a}`", s.name);
                    }
                }
                for v in &g.values {
                    let b = expr::unhex(v).ok_or_else(|| anyhow!("generate.vars.{}.values: `{v}` is not hex", s.name))?;
                    if b.len() as u64 > s.vt.max_len {
                        bail!("generate.vars.{}.values: `{v}` has {} bytes, more than max_len {}", s.name, b.len(), s.vt.max_len);
                    }
                }
            }
        }
    }
    // Order slots so that variables referenced by `len`/`max` come first.
    let deps = |s: &VarSlot| -> Result<Vec<String>> {
        let mut d = Vec::new();
        for src in [&s.generator.len, &s.generator.max].into_iter().flatten() {
            let e = expr::parse(src).map_err(|m| anyhow!("generate.vars.{}: {m}", s.name))?;
            collect_paths(&e, &mut d);
        }
        Ok(d)
    };
    let mut ordered: Vec<VarSlot> = Vec::new();
    let mut pending = out;
    while !pending.is_empty() {
        let before = pending.len();
        let mut i = 0;
        while i < pending.len() {
            let d = deps(&pending[i])?;
            if d.iter().all(|k| ordered.iter().any(|o| &o.key == k) || !pending.iter().any(|p| &p.key == k)) {
                ordered.push(pending.remove(i));
            } else {
                i += 1;
            }
        }
        if pending.len() == before {
            bail!("generate.vars: circular dependency among {:?}", pending.iter().map(|p| &p.name).collect::<Vec<_>>());
        }
    }
    Ok(ordered)
}

fn collect_paths(e: &expr::Expr, out: &mut Vec<String>) {
    use expr::Expr::*;
    match e {
        Path(p) => out.push(p.join(".")),
        Lit(_) | Int(_) => {}
        Unary(_, a) => collect_paths(a, out),
        Binary(_, a, b) | Index(a, b) => {
            collect_paths(a, out);
            collect_paths(b, out);
        }
        Call(_, args) => args.iter().for_each(|a| collect_paths(a, out)),
        Forall(_, a, b, c) | Count(_, a, b, c) => {
            collect_paths(a, out);
            collect_paths(b, out);
            collect_paths(c, out);
        }
    }
}

pub fn bv_boundaries(w: u8) -> Vec<u64> {
    let m = mask(w);
    let smax = m >> 1;
    let smin = smax + 1;
    let pattern = 0x0101_0101_0101_0101u64 & m;
    let mut v = vec![0, 1, 2, smax, smin, m - 1, m, pattern];
    let mut seen = Vec::new();
    v.retain(|x| {
        if seen.contains(x) {
            false
        } else {
            seen.push(*x);
            true
        }
    });
    v
}

fn parse_bv_literal(s: &str, w: u8) -> Result<u64> {
    let v = if let Some(h) = s.strip_prefix("0x") {
        u64::from_str_radix(&h.replace('_', ""), 16)?
    } else if let Some(d) = s.strip_prefix('-') {
        (d.parse::<u64>()? as i64).wrapping_neg() as u64 & mask(w)
    } else {
        s.parse::<u64>()?
    };
    if v & !mask(w) != 0 {
        bail!("value {s} does not fit in bv{w}");
    }
    Ok(v)
}

fn random_bv(rng: &mut SplitMix64, w: u8) -> u64 {
    let m = mask(w);
    match rng.below(8) {
        0..=3 => rng.next() & m,
        4 | 5 => rng.below(17) & m,
        6 => m.wrapping_sub(rng.below(17)) & m,
        _ => ((m >> 1).wrapping_add(rng.below(5)).wrapping_sub(2)) & m,
    }
}

fn random_bytes(rng: &mut SplitMix64, len: u64, alphabet: Option<&str>) -> Vec<u8> {
    (0..len)
        .map(|_| match alphabet {
            Some("nonzero") => 1 + rng.below(255) as u8,
            Some("ascii") => 0x20 + rng.below(0x5f) as u8,
            _ => rng.next() as u8,
        })
        .collect()
}

fn eval_max(slot: &VarSlot, env: &ValEnv) -> Result<Option<u64>> {
    let Some(src) = &slot.generator.max else { return Ok(None) };
    let e = expr::parse(src).map_err(|m| anyhow!("generate.vars.{}.max: {m}", slot.name))?;
    let empty = BTreeMap::new();
    match expr::eval(&e, &EvalCtx { vars: env, region_addrs: &empty }) {
        Ok(Value::Bv(_, n)) => Ok(Some(n)),
        Ok(v) => bail!("generate.vars.{}.max must be a bitvector, got {}", slot.name, v.ty()),
        Err(m) => bail!("generate.vars.{}.max: {m}", slot.name),
    }
}

fn check_len(slot: &VarSlot, n: u64) -> Result<()> {
    if n > slot.vt.max_len {
        bail!("PLAN_ERROR: generate.vars.{}.len evaluated to {n}, more than max_len {} in the contract", slot.name, slot.vt.max_len);
    }
    Ok(())
}

fn eval_len(slot: &VarSlot, env: &ValEnv) -> Result<Option<u64>> {
    let Some(src) = &slot.generator.len else { return Ok(None) };
    let e = expr::parse(src).map_err(|m| anyhow!("generate.vars.{}.len: {m}", slot.name))?;
    let empty = BTreeMap::new();
    match expr::eval(&e, &EvalCtx { vars: env, region_addrs: &empty }) {
        Ok(Value::Bv(_, n)) => Ok(Some(n)),
        Ok(v) => bail!("generate.vars.{}.len must be a bitvector, got {}", slot.name, v.ty()),
        Err(m) => bail!("generate.vars.{}.len: {m}", slot.name),
    }
}

/// Boundary candidates for one variable, given values of earlier variables.
fn candidates(slot: &VarSlot, env: &ValEnv, rng: &mut SplitMix64) -> Result<Vec<Value>> {
    match slot.vt.ty {
        Ty::Bool => Ok(vec![Value::Bool(false), Value::Bool(true)]),
        Ty::Bv(w) => {
            let max = eval_max(slot, env)?;
            let mut v: Vec<u64> = match max {
                Some(m) => {
                    let mut c = vec![0, 1, m / 2, m.saturating_sub(1), m];
                    c.retain(|x| *x <= m);
                    c.dedup();
                    c
                }
                None => bv_boundaries(w),
            };
            for s in &slot.generator.values {
                let x = parse_bv_literal(s, w).map_err(|e| anyhow!("generate.vars.{}.values: {e}", slot.name))?;
                if !v.contains(&x) && max.is_none_or(|m| x <= m) {
                    v.push(x);
                }
            }
            Ok(v.into_iter().map(|x| Value::Bv(w, x)).collect())
        }
        Ty::Bytes => {
            let mut out = Vec::new();
            for s in &slot.generator.values {
                let b = expr::unhex(s).ok_or_else(|| anyhow!("generate.vars.{}.values: `{s}` is not hex", slot.name))?;
                out.push(Value::Bytes(b));
            }
            let lens: Vec<u64> = match eval_len(slot, env)? {
                Some(n) => vec![n],
                None => {
                    let m = slot.vt.max_len;
                    let mut l = vec![0, 1, m / 2, m];
                    l.dedup();
                    l
                }
            };
            for n in lens {
                check_len(slot, n)?;
                out.push(Value::Bytes(random_bytes(rng, n, slot.generator.bytes.as_deref())));
            }
            Ok(out)
        }
    }
}

fn random_value(slot: &VarSlot, env: &ValEnv, rng: &mut SplitMix64) -> Result<Value> {
    Ok(match slot.vt.ty {
        Ty::Bool => Value::Bool(rng.below(2) == 1),
        Ty::Bv(w) => match eval_max(slot, env)? {
            Some(m) => Value::Bv(w, if m == u64::MAX { rng.next() } else { rng.below(m + 1) } & mask(w)),
            None => Value::Bv(w, random_bv(rng, w)),
        },
        Ty::Bytes => {
            let n = match eval_len(slot, env)? {
                Some(n) => {
                    check_len(slot, n)?;
                    n
                }
                None => rng.below(slot.vt.max_len + 1),
            };
            Value::Bytes(random_bytes(rng, n, slot.generator.bytes.as_deref()))
        }
    })
}

fn case_key(env: &ValEnv) -> String {
    env.iter().map(|(k, v)| format!("{k}={v};")).collect()
}

pub struct Generated {
    pub cases: Vec<Case>,
    pub stats: PlanStats,
}

pub fn generate(contract: &Contract, suite: &Suite, regressions: Vec<Case>, regression_na: usize) -> Result<Generated> {
    let slots = slots(contract, suite)?;
    let mut rng = SplitMix64::new(suite.seed);
    let mut stats = PlanStats { regression_not_applicable: regression_na, ..Default::default() };
    let mut cases: Vec<Case> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let empty = BTreeMap::new();

    let mut accept = |env: ValEnv, origin: Origin, rng: &mut SplitMix64, stats: &mut PlanStats, cases: &mut Vec<Case>| {
        for r in &contract.requires {
            match expr::eval(&r.expr, &EvalCtx { vars: &env, region_addrs: &empty }) {
                Ok(Value::Bool(true)) => {}
                Ok(_) => {
                    stats.excluded_by_requires += 1;
                    return;
                }
                Err(_) => {
                    stats.requires_eval_errors += 1;
                    return;
                }
            }
        }
        if !seen.insert(case_key(&env)) {
            stats.duplicate_inputs += 1;
        }
        let n = cases.len();
        let prefix = match origin {
            Origin::Regression => "reg",
            Origin::Boundary => "bnd",
            Origin::Random => "rnd",
        };
        match origin {
            Origin::Regression => stats.regression_cases += 1,
            Origin::Boundary => stats.boundary_cases += 1,
            Origin::Random => stats.random_cases += 1,
        }
        cases.push(Case { id: format!("{prefix}-{n:05}"), origin, values: env, filler_seed: rng.next() });
    };

    for r in regressions {
        accept(r.values, Origin::Regression, &mut rng, &mut stats, &mut cases);
    }

    if suite.boundary_product && !slots.is_empty() {
        // Cartesian product of per-variable candidates, built left to right so
        // that length expressions can refer to earlier variables.
        let mut partial: Vec<ValEnv> = vec![ValEnv::new()];
        let budget = (suite.limits.max_cases / 2).max(1) as usize;
        let mut too_big = false;
        for s in &slots {
            let mut next = Vec::new();
            for env in &partial {
                for v in candidates(s, env, &mut rng)? {
                    let mut e = env.clone();
                    e.insert(s.key.clone(), v);
                    next.push(e);
                }
            }
            if next.len() > budget {
                too_big = true;
                break;
            }
            partial = next;
        }
        if too_big {
            // One variable at a time at its boundaries, the others random.
            for (i, s) in slots.iter().enumerate() {
                let mut base = ValEnv::new();
                for t in &slots[..i] {
                    let v = random_value(t, &base, &mut rng)?;
                    base.insert(t.key.clone(), v);
                }
                for v in candidates(s, &base, &mut rng)? {
                    let mut env = base.clone();
                    env.insert(s.key.clone(), v);
                    for t in &slots[i + 1..] {
                        let v = random_value(t, &env, &mut rng)?;
                        env.insert(t.key.clone(), v);
                    }
                    accept(env, Origin::Boundary, &mut rng, &mut stats, &mut cases);
                }
            }
        } else {
            for env in partial {
                accept(env, Origin::Boundary, &mut rng, &mut stats, &mut cases);
            }
        }
    }

    for _ in 0..suite.random_cases {
        let mut env = ValEnv::new();
        for s in &slots {
            let v = random_value(s, &env, &mut rng)?;
            env.insert(s.key.clone(), v);
        }
        accept(env, Origin::Random, &mut rng, &mut stats, &mut cases);
    }

    if cases.len() as u64 > suite.limits.max_cases {
        bail!(
            "PLAN_LIMIT_EXCEEDED: {} cases planned, limit is {} (reduce generate.random_cases; cases are never dropped silently)",
            cases.len(),
            suite.limits.max_cases
        );
    }
    Ok(Generated { cases, stats })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_are_hand_fixed() {
        assert_eq!(bv_boundaries(8), vec![0, 1, 2, 0x7f, 0x80, 0xfe, 0xff]);
        assert_eq!(
            bv_boundaries(64),
            vec![0, 1, 2, 0x7fff_ffff_ffff_ffff, 0x8000_0000_0000_0000, u64::MAX - 1, u64::MAX, 0x0101_0101_0101_0101]
        );
    }

    #[test]
    fn splitmix_is_stable() {
        // Reference values of SplitMix64 with seed 0 (Vigna's reference implementation).
        let mut r = SplitMix64::new(0);
        assert_eq!(r.next(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(r.next(), 0x6e78_9e6a_a1b9_65f4);
    }
}
