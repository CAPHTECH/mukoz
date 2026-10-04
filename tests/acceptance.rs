//! Fixture expectations written before running them (docs/11 M02).
//! Each mutated fixture must be rejected for the announced property (I4),
//! and unsupported/budget cases must be HOLD, never ACCEPT (I1).

use serde_json::Value;
use std::process::Command;

fn check(suite: &str, artifact: &str, extra: &[&str]) -> Value {
    let store = tempdir();
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz"))
        .args(["check", suite, "--artifact", artifact, "--store", &store])
        .args(extra)
        .output()
        .expect("run mukoz");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("bad json: {e}\n{}", String::from_utf8_lossy(&out.stdout)));
    let _ = std::fs::remove_dir_all(&store);
    v
}

fn tempdir() -> String {
    let p = std::env::temp_dir().join(format!("mukoz-test-{}-{}", std::process::id(), rand_suffix()));
    p.display().to_string()
}

fn rand_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::SeqCst)
}

fn admission(v: &Value) -> &str {
    v["data"]["assessment"]["admission"].as_str().unwrap_or_else(|| panic!("no admission: {v:#}"))
}

fn violated(v: &Value) -> Vec<String> {
    v["data"]["claims"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["evaluation"] == "VIOLATED")
        .map(|c| c["property"].as_str().unwrap().to_string())
        .collect()
}

const ADD: &str = "examples/add64/suite.x86_64.toml";
fn fx(n: &str) -> String {
    format!("fixtures/x86_64/{n}.bin")
}

#[test]
fn correct_variants_are_accepted() {
    for n in ["add64", "add64_push", "add64_nested", "add64_redzone"] {
        let v = check(ADD, &fx(n), &[]);
        assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "{n}: {:#}", v["data"]["assessment"]);
        assert_eq!(v["data"]["assessment"]["scope"]["cases_completed"], 4160, "{n}");
    }
}

#[test]
fn mutations_are_rejected_with_the_announced_property() {
    let expect = [
        ("add64_mut_sub", "arith.add64/sum"),
        ("add64_mut_32bit", "arith.add64/sum"),
        ("add64_mut_clobber_rbx", "machine.abi.callee_saved"),
        ("add64_mut_wild_write", "machine.memory.access"),
        ("add64_mut_syscall", "effects.no_forbidden"),
    ];
    for (n, prop) in expect {
        let v = check(ADD, &fx(n), &[]);
        assert_eq!(admission(&v), "REJECT", "{n}");
        assert!(violated(&v).contains(&prop.to_string()), "{n}: expected {prop}, got {:?}", violated(&v));
        let f = &v["data"]["findings"][0];
        assert!(f["counterexample_id"].as_str().unwrap().starts_with("cx-"), "{n}");
    }
}

#[test]
fn budget_and_unsupported_are_hold_not_accept() {
    for (n, reason) in [("add64_mut_loop", "BUDGET_EXHAUSTED"), ("add64_mut_ud2", "UNSUPPORTED_DURING_RUN")] {
        let v = check(ADD, &fx(n), &["--fail-fast"]);
        assert_eq!(admission(&v), "HOLD", "{n}");
        let reasons = v["data"]["assessment"]["reasons"].to_string();
        assert!(reasons.contains(reason), "{n}: {reasons}");
    }
}

#[test]
fn fail_fast_rejects_without_accepting_skipped_cases() {
    let v = check(ADD, &fx("add64_mut_sub"), &["--fail-fast"]);
    assert_eq!(admission(&v), "REJECT");
    assert!(v["data"]["assessment"]["scope"]["cases_skipped"].as_u64().unwrap() > 0);
}

#[test]
fn memory_tasks_correct_are_accepted() {
    for (suite, art) in [
        ("examples/copy/suite.x86_64.toml", "copy"),
        ("examples/strlen/suite.x86_64.toml", "strlen"),
        ("examples/checked_inc/suite.x86_64.toml", "checked_inc"),
    ] {
        let v = check(suite, &fx(art), &[]);
        assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "{art}: {:#}", v["data"]["assessment"]);
    }
}

#[test]
fn memory_task_mutations_are_rejected() {
    let expect = [
        ("examples/copy/suite.x86_64.toml", "copy_mut_offbyone", "machine.memory.access"),
        ("examples/copy/suite.x86_64.toml", "copy_mut_half", "mem.copy/copied"),
        ("examples/strlen/suite.x86_64.toml", "strlen_mut_count_nul", "str.len/length"),
        ("examples/checked_inc/suite.x86_64.toml", "checked_inc_mut_nocheck", "counter.checked_inc/ok_iff_no_overflow"),
    ];
    for (suite, n, prop) in expect {
        let v = check(suite, &fx(n), &[]);
        assert_eq!(admission(&v), "REJECT", "{n}");
        assert!(violated(&v).contains(&prop.to_string()), "{n}: expected {prop}, got {:?}", violated(&v));
    }
}

#[test]
fn same_contract_other_isa() {
    let v = check("examples/add64/suite.aarch64.toml", "fixtures/aarch64/add64.bin", &[]);
    assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "{:#}", v["data"]["assessment"]);
    let v = check("examples/add64/suite.aarch64.toml", "fixtures/aarch64/add64_mut_sub.bin", &[]);
    assert_eq!(admission(&v), "REJECT");
    assert!(violated(&v).contains(&"arith.add64/sum".to_string()));
}

#[test]
fn narrow_arguments_have_unspecified_upper_bits() {
    let v = check("examples/add32/suite.x86_64.toml", &fx("add32"), &[]);
    assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "{:#}", v["data"]["assessment"]);
    let v = check("examples/zext32/suite.x86_64.toml", &fx("zext32"), &[]);
    assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "{:#}", v["data"]["assessment"]);
    // `mov rax, rdi` is correct only if the caller zeroed the upper 32 bits.
    let v = check("examples/zext32/suite.x86_64.toml", &fx("zext32_mut_upper_bits"), &[]);
    assert_eq!(admission(&v), "REJECT");
    assert!(violated(&v).contains(&"arith.zext32/zero_extended".to_string()));
}

// I2: a counterexample replayed against a changed artifact is reported as a
// regression check on a different subject, not as the original verdict.
#[test]
fn i2_replay_on_changed_artifact_is_marked() {
    let store = tempdir();
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz"))
        .args(["check", ADD, "--artifact", &fx("add64_mut_sub"), "--store", &store, "--fail-fast"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let cx = v["data"]["findings"][0]["counterexample_id"].as_str().unwrap().to_string();
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz"))
        .args(["replay", &cx, "--artifact", &fx("add64"), "--store", &store])
        .output()
        .unwrap();
    let r: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["data"]["same_artifact_contract_binding"], false);
    assert_eq!(r["data"]["property_now"], "SATISFIED_IN_SCOPE");
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn gate_exit_codes_only_with_flag() {
    let store = tempdir();
    let run = |art: &str, gate: bool| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_mukoz"));
        c.args(["check", ADD, "--artifact", art, "--store", &store, "--fail-fast"]);
        if gate {
            c.arg("--gate");
        }
        c.output().unwrap().status.code().unwrap()
    };
    assert_eq!(run(&fx("add64_mut_sub"), false), 0);
    assert_eq!(run(&fx("add64_mut_sub"), true), 11);
    assert_eq!(run(&fx("add64_mut_loop"), true), 10);
    assert_eq!(run(&fx("add64"), true), 0);
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn missing_artifact_is_a_usage_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz")).args(["check", "eval/tasks/smax/suite.toml", "--store", &tempdir()]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
}

/// Writes a variant of the add64 example (contract + suite) into a temp dir.
fn add64_variant(extra_contract: &str, extra_suite: &str) -> (String, String) {
    let dir = tempdir();
    std::fs::create_dir_all(&dir).unwrap();
    let ex = std::fs::canonicalize("examples/add64").unwrap();
    let c = std::fs::read_to_string(ex.join("contract.toml")).unwrap() + extra_contract;
    std::fs::write(format!("{dir}/contract.toml"), c).unwrap();
    std::fs::copy(ex.join("binding.x86_64.toml"), format!("{dir}/binding.toml")).unwrap();
    let s = format!(
        "schema = \"mukoz.suite/1\"\nid = \"t\"\ncontract = \"contract.toml\"\nbinding = \"binding.toml\"\n\n[generate]\nseed = \"1\"\nrandom_cases = 1024\n{extra_suite}"
    );
    std::fs::write(format!("{dir}/suite.toml"), s).unwrap();
    (dir.clone(), format!("{dir}/suite.toml"))
}

// Found by an agent writing a contract: `requires` dropped 525 of 528 cases and the
// result was still ACCEPT_WITHIN_SCOPE. A near-vacuous scope must be HOLD (I5).
#[test]
fn requires_excluding_most_cases_is_hold() {
    let (dir, suite) = add64_variant("\n[[requires]]\nid = \"rare\"\nexpr = \"input.a == bv64(5)\"\n", "");
    let v = check(&suite, &fx("add64"), &[]);
    assert_eq!(admission(&v), "HOLD", "{:#}", v["data"]["assessment"]);
    let reasons = v["data"]["assessment"]["reasons"].to_string();
    assert!(reasons.contains("LOW_ADMITTED_CASES"), "{reasons}");
    // An explicit, deliberate floor restores acceptance.
    let (dir2, suite2) = add64_variant("\n[[requires]]\nid = \"rare\"\nexpr = \"input.a == bv64(5)\"\n", "\n[limits]\nmin_admitted_cases = 1\n");
    assert_eq!(admission(&check(&suite2, &fx("add64"), &[])), "ACCEPT_WITHIN_SCOPE");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(dir2);
}

// Generator fields that do not apply are errors, not silently ignored.
#[test]
fn inapplicable_generator_fields_are_errors() {
    let (dir, suite) = add64_variant("", "\n[generate.vars.a]\nlen = \"bv64(3)\"\n");
    let v = check(&suite, &fx("add64"), &[]);
    assert_eq!(v["ok"], false, "{v:#}");
    assert!(v["errors"].to_string().contains("does not apply"), "{v:#}");
    let _ = std::fs::remove_dir_all(dir);
}

// Found by an agent writing a contract: a boundary product over the budget silently fell back
// to one-variable-at-a-time. The fallback must be visible in limitations and plan_stats.
#[test]
fn boundary_product_fallback_is_reported() {
    let vals: Vec<String> = (0..80).map(|i| format!("\"{i}\"")).collect();
    let extra = format!("\n[generate.vars.a]\nvalues = [{}]\n\n[generate.vars.b]\nvalues = [{}]\n", vals.join(", "), vals.join(", "));
    let (dir, suite) = add64_variant("", &extra);
    let v = check(&suite, &fx("add64"), &[]);
    assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "{:#}", v["data"]["assessment"]);
    let scope = &v["data"]["assessment"]["scope"];
    assert_eq!(scope["plan_stats"]["boundary_mode"], "one_at_a_time", "{scope:#}");
    assert!(v["data"]["assessment"]["limitations"].to_string().contains("boundary_product_reduced"), "{:#}", v["data"]["assessment"]);
    // The input summary shows what was generated.
    assert_eq!(scope["input_summary"]["input.a"]["type"], "bv64", "{scope:#}");
    let _ = std::fs::remove_dir_all(dir);
}
