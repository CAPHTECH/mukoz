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
