//! Engine qualification (docs/02 2.5, docs/03 3.6 rule 4): known-answer instruction tests run
//! through the same emulated executor (harness included) that `check` uses. Emulated results are
//! adopted only while a passing EngineQualification for this host, engine build, ISA and test
//! set is stored; otherwise the run is HOLD. On an x86_64 Linux host the x86_64 vectors are also
//! run on the real CPU (native-routine) as a cross-check of the reference values.

use crate::emu::{self, Executor, Stop};
use crate::expr::Value;
use crate::image::Image;
use crate::plan::{Case, Origin};
use crate::spec::{Binding, Contract, Isa};
use crate::store::Store;
use anyhow::Result;
use serde_json::{Value as J, json};

pub struct Test {
    pub name: &'static str,
    /// An engine may not implement this instruction (Unicorn's default CPU model has no
    /// POPCNT): the emulator must either stop with INVALID_INSTRUCTION or give the expected
    /// values. A wrong value fails either way.
    pub emulated_unsupported: bool,
    pub code: &'static [u8],
    /// (a, b, expected first result register, expected second result register)
    pub vectors: &'static [(u64, u64, u64, Option<u64>)],
}

pub const SCHEMA: &str = "mukoz.qualification/1";

fn isa_name(isa: Isa) -> &'static str {
    match isa {
        Isa::X86_64 => "x86_64",
        Isa::Aarch64 => "aarch64",
    }
}

pub fn record_name(isa: Isa) -> String {
    format!("qualification-{}", isa_name(isa))
}

fn harness(isa: Isa) -> Result<(Contract, Binding)> {
    let dir = crate::host::make_trial_dir("qualify");
    let (target, a, b, r0, r1) = match isa {
        Isa::X86_64 => ("x86_64/raw/sysv-x86_64/none", "rdi", "rsi", "rax", "rdx"),
        Isa::Aarch64 => ("aarch64/raw/aapcs64/none", "x0", "x1", "x0", "x1"),
    };
    std::fs::write(
        dir.join("contract.toml"),
        "schema = \"mukoz.contract/1\"\nid = \"mukoz.qualify\"\nboundary = \"routine\"\n[inputs]\na = \"bv64\"\nb = \"bv64\"\n[results]\nr0 = \"bv64\"\nr1 = \"bv64\"\n[effects]\nallow = []\n[termination]\nkind = \"must_return\"\n",
    )?;
    std::fs::write(
        dir.join("binding.toml"),
        format!(
            "schema = \"mukoz.binding/1\"\nid = \"mukoz.qualify@{target}\"\ncontract = \"mukoz.qualify\"\ntarget = \"{target}\"\n[entry]\nkind = \"raw_offset\"\noffset = 0\n[arguments]\n{a} = \"input.a\"\n{b} = \"input.b\"\n[results]\nr0 = \"{r0}\"\nr1 = \"{r1}\"\n[completion]\nkind = \"return_to_sentinel\"\n"
        ),
    )?;
    let c = Contract::load(&dir.join("contract.toml"))?;
    let bd = Binding::load(&dir.join("binding.toml"), &c, &emu::is_arg_or_result_reg)?;
    crate::host::remove_dir(&dir);
    Ok((c, bd))
}

fn case(i: usize, a: u64, b: u64) -> Case {
    let mut values = crate::expr::ValEnv::new();
    values.insert("input.a".into(), Value::bv(64, a));
    values.insert("input.b".into(), Value::bv(64, b));
    Case { id: format!("q{i}"), origin: Origin::Boundary, values, filler_seed: 0x5157_4c49_4659 ^ i as u64 }
}

/// Compare one observation with the expected values; None when it matches.
fn mismatch(isa: Isa, obs: &emu::Observation, o0: u64, o1: Option<u64>) -> Option<J> {
    let (r0, r1) = match isa {
        Isa::X86_64 => ("rax", "rdx"),
        Isa::Aarch64 => ("x0", "x1"),
    };
    if !matches!(obs.stop, Stop::Returned) {
        return Some(json!({ "stop": obs.stop }));
    }
    let g0 = obs.regs_out.get(r0).copied();
    let g1 = obs.regs_out.get(r1).copied();
    if g0 != Some(o0) || (o1.is_some() && g1 != o1) {
        return Some(json!({ r0: g0.map(|x| format!("0x{x:x}")), r1: g1.map(|x| format!("0x{x:x}")) }));
    }
    None
}

/// Run the test set for `isa` and return the EngineQualification record.
pub fn qualify(isa: Isa) -> Result<J> {
    let tests: &[Test] = match isa {
        Isa::X86_64 => crate::qualify_vectors::X86_64,
        Isa::Aarch64 => crate::qualify_vectors::AARCH64,
    };
    let (contract, binding) = harness(isa)?;
    let native_ok = isa == Isa::X86_64 && crate::native::host_can_execute(&binding).is_ok();
    let mut vectors = 0;
    let mut failures = Vec::new();
    let mut native_failures = Vec::new();
    let mut native_vectors = 0;
    let mut unsupported: Vec<&str> = Vec::new();
    for t in tests {
        let image = Image::raw(t.code, 0, &[])?;
        let exec = Executor { image: &image, contract: &contract, binding: &binding, insn_limit: 100_000, timeout_ms: 1000, vary_placement: false };
        for (i, &(a, b, o0, o1)) in t.vectors.iter().enumerate() {
            vectors += 1;
            let c = case(i, a, b);
            let obs = exec.run(&c);
            let bad = if t.emulated_unsupported && matches!(obs.stop, Stop::InvalidInstruction { .. }) {
                if !unsupported.contains(&t.name) {
                    unsupported.push(t.name);
                }
                None
            } else {
                mismatch(isa, &obs, o0, o1)
            };
            if let Some(got) = bad {
                failures.push(json!({ "test": t.name, "a": format!("0x{a:x}"), "b": format!("0x{b:x}"), "expected": [format!("0x{o0:x}"), o1.map(|x| format!("0x{x:x}"))], "got": got }));
            }
            if native_ok {
                native_vectors += 1;
                let n = crate::native::run(&image, &binding, &c, false, 1000);
                if let Some(got) = mismatch(isa, &n.obs, o0, o1) {
                    native_failures.push(json!({ "test": t.name, "a": format!("0x{a:x}"), "b": format!("0x{b:x}"), "expected": [format!("0x{o0:x}"), o1.map(|x| format!("0x{x:x}"))], "got": got }));
                }
            }
        }
    }
    let passed = failures.is_empty();
    Ok(json!({
        "schema": SCHEMA,
        "isa": isa_name(isa),
        "engine": emu::engine_id(),
        "mukoz": crate::run::EVALUATOR_VERSION,
        "host_id": crate::host::host_id(),
        "test_set": crate::qualify_vectors::TEST_SET,
        "tests": tests.len(),
        "vectors": vectors,
        "passed": passed,
        "failures": failures,
        "expected_values_from": "reference semantics in tools/qualify/gen.py (not the engine, not the expression evaluator)",
        "native_cross_check": if native_ok {
            json!({ "vectors": native_vectors, "passed": native_failures.is_empty(), "failures": native_failures })
        } else {
            json!({ "ran": false, "why": "the host cannot run this ISA natively" })
        },
        "unsupported_by_engine": unsupported,
        "scope": "the listed integer, flag, stack, call and loop instructions only; SIMD, floating point, atomics and system instructions are not qualified",
    }))
}

/// Does `q` describe this host, engine build, ISA and test set?
fn same_identity(q: &J, isa: Isa) -> Result<(), String> {
    let want = [
        ("schema", SCHEMA.to_string()),
        ("isa", isa_name(isa).to_string()),
        ("engine", emu::engine_id()),
        ("mukoz", crate::run::EVALUATOR_VERSION.to_string()),
        ("host_id", crate::host::host_id()),
        ("test_set", crate::qualify_vectors::TEST_SET.to_string()),
    ];
    for (k, v) in want {
        if q[k].as_str() != Some(v.as_str()) {
            return Err(format!("ENGINE_NOT_QUALIFIED: stored qualification has {k} = {}, this run needs {v}", q[k]));
        }
    }
    Ok(())
}

/// Is `q` a passing record for this host, engine build, ISA and test set?
pub fn valid(q: &J, isa: Isa) -> Result<(), String> {
    same_identity(q, isa)?;
    if q["passed"] != J::Bool(true) {
        return Err(format!(
            "ENGINE_NOT_QUALIFIED: {} of {} known-answer vectors failed on this host (see the record; rerun `mukoz platform qualify --isa {}` after fixing the cause)",
            q["failures"].as_array().map_or(0, |f| f.len()),
            q["vectors"],
            isa_name(isa)
        ));
    }
    Ok(())
}

/// The qualification `check` relies on. A stored record for this host, engine build, ISA and
/// test set is used as it is, failed or not (a failure is not re-rolled automatically); a
/// missing or outdated record is replaced by a fresh run. Returns the record, whether it was
/// run now, and Err if the engine is not qualified.
pub fn ensure(store: &Store, isa: Isa) -> (J, bool, Result<(), String>) {
    // Without the engine process nothing can be qualified, and nothing is stored.
    if let Err(e) = emu::engine() {
        return (J::Null, false, Err(format!("ENGINE_NOT_QUALIFIED: {e}")));
    }
    if let Some(q) = store.get_host(&record_name(isa)) {
        if same_identity(&q, isa).is_ok() {
            let v = valid(&q, isa);
            return (q, false, v);
        }
    }
    match qualify(isa) {
        Ok(q) => {
            let _ = store.put_host(&record_name(isa), &q);
            let v = valid(&q, isa);
            (q, true, v)
        }
        Err(e) => (J::Null, true, Err(format!("ENGINE_NOT_QUALIFIED: qualification could not run: {e:#}"))),
    }
}
