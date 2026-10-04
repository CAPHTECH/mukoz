//! native-routine, the owner policy and differential tests (docs/05 5.5, docs/06 6.8, docs/08 8.4).
//! Linux x86_64 hosts only.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const NATIVE: &str = "examples/add64/suite.native.x86_64.toml";
const DIFF: &str = "examples/add64/suite.diff.x86_64.toml";
const ISOLATION: &str = r#"["network_restriction", "filesystem_restriction", "resource_limits", "descendant_process_control", "credential_isolation"]"#;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tempdir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!("mukoz-native-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn mukoz(args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz")).args(args).current_dir(root()).output().expect("run mukoz");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("bad json: {e}\n{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// A policy whose single zone covers `dir` for the routine target.
fn policy(dir: &Path, target: &str, isolation: &str) -> PathBuf {
    let d = tempdir("policy");
    let p = d.join("policy.toml");
    std::fs::write(
        &p,
        format!(
            "[[native_trial_zones]]\nid = \"t\"\nartifact_dir = \"{}\"\nexecutors = [\"native-routine\"]\ntargets = [\"{target}\"]\nrequire_isolation = {isolation}\nmax_wall_ms_per_case = 1000\n",
            dir.display()
        ),
    )
    .unwrap();
    p
}

fn zone_policy() -> PathBuf {
    policy(&root().join("fixtures"), "x86_64/raw/sysv-x86_64/none", ISOLATION)
}

fn check(suite: &str, artifact: &str, policy: Option<&Path>, store: Option<&Path>, extra: &[&str]) -> Value {
    let st = store.map(Path::to_path_buf).unwrap_or_else(|| tempdir("store"));
    let mut a = vec!["check", suite, "--artifact", artifact, "--store", st.to_str().unwrap()];
    let pp;
    if let Some(p) = policy {
        pp = p.display().to_string();
        a.extend(["--policy", pp.as_str()]);
    }
    a.extend(extra);
    mukoz(&a)
}

fn claim<'a>(v: &'a Value, p: &str) -> &'a Value {
    v["data"]["claims"].as_array().unwrap().iter().find(|c| c["property"] == p).unwrap_or_else(|| panic!("no claim {p}: {v:#}"))
}

fn admission(v: &Value) -> &str {
    v["data"]["assessment"]["admission"].as_str().unwrap_or_else(|| panic!("no admission: {v:#}"))
}

#[test]
fn without_policy_nothing_runs_and_nothing_falls_back() {
    let v = check(NATIVE, "fixtures/x86_64/add64.bin", None, None, &[]);
    assert_eq!(admission(&v), "HOLD");
    for c in v["data"]["claims"].as_array().unwrap() {
        assert_eq!(c["evaluation"], "NOT_EVALUATED", "{c}");
        assert!(c["not_evaluated_reasons"]["NATIVE_NOT_PERMITTED"].as_u64().unwrap() > 0, "{c}");
    }
    let d = check(DIFF, "fixtures/x86_64/add64.bin", None, None, &[]);
    assert_eq!(admission(&d), "HOLD");
    assert_eq!(claim(&d, "differential.emulated_vs_native-routine")["evaluation"], "NOT_EVALUATED");
    assert_eq!(claim(&d, "arith.add64/sum")["evaluation"], "SATISFIED_IN_SCOPE");
}

#[test]
fn native_alone_cannot_establish_the_memory_claim() {
    let v = check(NATIVE, "fixtures/x86_64/add64.bin", Some(&zone_policy()), None, &[]);
    assert_eq!(admission(&v), "HOLD");
    assert_eq!(claim(&v, "arith.add64/sum")["evaluation"], "SATISFIED_IN_SCOPE");
    assert_eq!(claim(&v, "machine.abi.callee_saved")["evaluation"], "SATISFIED_IN_SCOPE");
    let m = claim(&v, "machine.memory.access");
    assert_eq!(m["evaluation"], "NOT_EVALUATED");
    assert!(m["not_evaluated_reasons"]["REQUIRED_CAPABILITY_UNAVAILABLE"].as_u64().unwrap() > 0);
    let p = &v["data"]["assessment"]["scope"]["platform"]["native-routine"];
    assert_eq!(p["permitted_by"], "zone:t");
    assert!(p["isolation_applied"].as_array().unwrap().iter().any(|x| x == "network_restriction"), "{p}");
}

#[test]
fn native_mutants_are_rejected_for_the_announced_property() {
    let pol = zone_policy();
    for (m, prop) in [
        ("add64_mut_sub", "arith.add64/sum"),
        ("add64_mut_clobber_rbx", "machine.abi.callee_saved"),
        ("add64_mut_syscall", "effects.no_forbidden"),
        ("add64_mut_wild_write", "machine.memory.access"),
    ] {
        let v = check(NATIVE, &format!("fixtures/x86_64/{m}.bin"), Some(&pol), None, &["--fail-fast"]);
        assert_eq!(admission(&v), "REJECT", "{m}");
        assert_eq!(claim(&v, prop)["evaluation"], "VIOLATED", "{m}");
    }
    let v = check(NATIVE, "fixtures/x86_64/add64_mut_ud2.bin", Some(&pol), None, &[]);
    assert_eq!(admission(&v), "HOLD");
    assert!(claim(&v, "machine.returned")["inconclusive_reasons"]["UNSUPPORTED_DURING_RUN"].as_u64().unwrap() > 0);
}

#[test]
fn nonterminating_native_cases_stop_after_a_few_and_hold() {
    let v = check(NATIVE, "fixtures/x86_64/add64_mut_loop.bin", Some(&zone_policy()), None, &[]);
    assert_eq!(admission(&v), "HOLD");
    assert!(v["data"]["assessment"]["limitations"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().starts_with("native_cases_stopped_after")));
}

#[test]
fn differential_agrees_on_add64_and_reports_divergence_on_cpuid() {
    let pol = zone_policy();
    let v = check(DIFF, "fixtures/x86_64/add64.bin", Some(&pol), None, &[]);
    assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "{:#}", v["data"]["assessment"]);
    assert_eq!(claim(&v, "differential.emulated_vs_native-routine")["evaluation"], "SATISFIED_IN_SCOPE");

    let st = tempdir("store");
    let d = check(DIFF, "fixtures/x86_64/cpuid_sig.bin", Some(&pol), Some(&st), &[]);
    let c = claim(&d, "differential.emulated_vs_native-routine");
    assert_eq!(c["evaluation"], "INCONCLUSIVE");
    assert!(c["inconclusive_reasons"]["BACKEND_DIVERGENCE"].as_u64().unwrap() > 0);
    let id = c["counterexamples"][0].as_str().unwrap();
    let s = mukoz(&["show", id, "--store", st.to_str().unwrap()]);
    assert_eq!(s["data"]["kind"], "divergence");
    assert_eq!(s["data"]["detail"]["differences"][0]["register"], "rax");
}

#[test]
fn a_missing_isolation_capability_blocks_native_execution() {
    let st = tempdir("store");
    let p = mukoz(&["platform", "probe", "--store", st.to_str().unwrap()]);
    let mut probe = p["data"].clone();
    probe["capabilities"]["network_restriction"]["confirmed"] = Value::Bool(false);
    std::fs::write(st.join("host/probe.json"), serde_json::to_vec(&probe).unwrap()).unwrap();
    let v = check(NATIVE, "fixtures/x86_64/add64.bin", Some(&zone_policy()), Some(&st), &[]);
    assert_eq!(admission(&v), "HOLD");
    let why = v["data"]["assessment"]["scope"]["platform"]["native-routine"]["not_run"].as_str().unwrap();
    assert!(why.starts_with("NATIVE_NOT_PERMITTED") && why.contains("network_restriction"), "{why}");
    assert_eq!(claim(&v, "arith.add64/sum")["evaluation"], "NOT_EVALUATED");
}

#[test]
fn zone_rejects_symlinks_and_files_outside_and_unlisted_targets() {
    let zone = tempdir("zone");
    std::os::unix::fs::symlink(root().join("fixtures/x86_64/add64.bin"), zone.join("link.bin")).unwrap();
    let pol = policy(&zone, "x86_64/raw/sysv-x86_64/none", ISOLATION);
    for art in [zone.join("link.bin"), root().join("fixtures/x86_64/add64.bin")] {
        let v = check(NATIVE, art.to_str().unwrap(), Some(&pol), None, &[]);
        let why = v["data"]["assessment"]["scope"]["platform"]["native-routine"]["not_run"].as_str().unwrap();
        assert!(why.starts_with("NATIVE_NOT_PERMITTED"), "{why}");
    }
    std::fs::copy(root().join("fixtures/x86_64/add64.bin"), zone.join("add64.bin")).unwrap();
    let other = policy(&zone, "x86_64/elf/sysv-x86_64/linux", ISOLATION);
    let v = check(NATIVE, zone.join("add64.bin").to_str().unwrap(), Some(&other), None, &[]);
    assert!(v["data"]["assessment"]["scope"]["platform"]["native-routine"]["not_run"].as_str().unwrap().contains("not listed"));
    let v = check(NATIVE, zone.join("add64.bin").to_str().unwrap(), Some(&pol), None, &[]);
    assert_eq!(v["data"]["assessment"]["scope"]["platform"]["native-routine"]["permitted_by"], "zone:t");
}
