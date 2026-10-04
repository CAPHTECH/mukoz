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

// ---------------------------------------------------------------- native-process

fn process_policy() -> PathBuf {
    let d = tempdir("ppolicy");
    let p = d.join("policy.toml");
    std::fs::write(
        &p,
        format!(
            "[[native_trial_zones]]\nid = \"p\"\nartifact_dir = \"{}\"\nexecutors = [\"native-process\"]\ntargets = [\"x86_64/elf/sysv-x86_64/linux\", \"x86_64/raw/sysv-x86_64/linux\", \"aarch64/elf/aapcs64/linux\"]\nrequire_isolation = {ISOLATION}\nmax_wall_ms_per_case = 2000\n",
            root().join("fixtures").display()
        ),
    )
    .unwrap();
    p
}

#[test]
fn hello_elf_is_accepted_by_emulated_and_native_process_together() {
    let pol = process_policy();
    for art in ["hello_x86.elf", "hello_x86_eq.elf"] {
        let v = check("examples/hello/suite.x86_64.elf.diff.toml", &format!("fixtures/process/{art}"), Some(&pol), None, &[]);
        assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "{art}: {:#}", v["data"]["assessment"]);
        assert_eq!(claim(&v, "differential.emulated_vs_native-process")["evaluation"], "SATISFIED_IN_SCOPE");
    }
    for suite in ["examples/hello/suite.x86_64.elf.diff.toml", "examples/hello/suite.x86_64.elf.native.toml"] {
        let v = check(suite, "fixtures/process/hello_x86_mut_len.elf", Some(&pol), None, &[]);
        assert_eq!(admission(&v), "REJECT", "{suite}");
        assert_eq!(claim(&v, "proc.hello/greets")["evaluation"], "VIOLATED");
    }
    let v = check("examples/hello/suite.x86_64.raw.diff.toml", "fixtures/process/hello_x86.bin", Some(&pol), None, &[]);
    assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "raw image through the wrapped ELF");
}

#[test]
fn native_process_alone_claims_no_memory_or_effect_guarantee() {
    let v = check("examples/hello/suite.x86_64.elf.native.toml", "fixtures/process/hello_x86.elf", Some(&process_policy()), None, &[]);
    assert_eq!(admission(&v), "HOLD");
    for p in ["machine.memory.access", "effects.no_forbidden"] {
        let c = claim(&v, p);
        assert_eq!(c["evaluation"], "NOT_EVALUATED", "{p}");
        assert!(c["not_evaluated_reasons"]["REQUIRED_CAPABILITY_UNAVAILABLE"].as_u64().unwrap() > 0);
    }
    for p in ["machine.exited", "effects.output_within_limit", "proc.hello/greets", "proc.hello/exit_zero"] {
        assert_eq!(claim(&v, p)["evaluation"], "SATISFIED_IN_SCOPE", "{p}");
    }
    let reasons = v["data"]["assessment"]["reasons"].as_array().unwrap();
    assert_eq!(reasons.len(), 2, "{reasons:?}");
}

#[test]
fn todo_files_argv_and_exit_status_agree_natively() {
    let pol = process_policy();
    let v = check("examples/todo/suite.x86_64.elf.diff.toml", "fixtures/process/todo_x86.elf", Some(&pol), None, &[]);
    assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "{:#}", v["data"]["assessment"]);
    for m in ["todo_x86_mut_nonl.elf", "todo_x86_mut_noappend.elf"] {
        let v = check("examples/todo/suite.x86_64.elf.native.toml", &format!("fixtures/process/{m}"), Some(&pol), None, &[]);
        assert_eq!(admission(&v), "REJECT", "{m}");
        assert_eq!(claim(&v, "proc.todo/add_appends_line")["evaluation"], "VIOLATED", "{m}");
    }
    // fstat is outside the effect model: the emulated side holds, and the difference is not
    // reported as a backend divergence.
    let v = check("examples/todo/suite.x86_64.elf.diff.toml", "fixtures/process/todo_x86_mut_stat.elf", Some(&pol), None, &[]);
    assert_eq!(admission(&v), "HOLD");
    let d = claim(&v, "differential.emulated_vs_native-process");
    assert_eq!(d["evaluation"], "NOT_EVALUATED");
    assert!(d["not_evaluated_reasons"]["EMULATED_UNSUPPORTED"].as_u64().unwrap() > 0);
}

#[test]
fn a_foreign_isa_is_never_run_natively() {
    let v = check("examples/hello/suite.aarch64.elf.toml", "fixtures/process/hello_a64.elf", Some(&process_policy()), None, &[]);
    assert_eq!(admission(&v), "ACCEPT_WITHIN_SCOPE", "emulated alone");
    let st = tempdir("a64");
    let suite = st.join("suite.toml");
    let text = std::fs::read_to_string(root().join("examples/hello/suite.aarch64.elf.toml")).unwrap();
    let text = text.replace("contract = \"contract.toml\"", &format!("contract = \"{}\"", root().join("examples/hello/contract.toml").display()));
    let text = text.replace("binding = \"binding.aarch64.elf.toml\"", &format!("binding = \"{}\"\nexecutors = [\"emulated\", \"native-process\"]", root().join("examples/hello/binding.aarch64.elf.toml").display()));
    std::fs::write(&suite, text).unwrap();
    let v = check(suite.to_str().unwrap(), "fixtures/process/hello_a64.elf", Some(&process_policy()), None, &[]);
    assert_eq!(admission(&v), "HOLD");
    let d = claim(&v, "differential.emulated_vs_native-process");
    assert!(d["not_evaluated_reasons"]["HOST_CANNOT_EXECUTE_TARGET"].as_u64().unwrap() > 0, "{d}");
}

#[test]
fn a_dynamically_linked_elf_is_an_unresolved_dependency_not_a_violation() {
    let v = check("examples/hello/suite.x86_64.elf.diff.toml", "fixtures/process/hello_dyn.elf", Some(&process_policy()), None, &[]);
    assert_eq!(v["ok"], false);
    assert_eq!(v["errors"][0]["code"], "UNRESOLVED_DEPENDENCY");
}
