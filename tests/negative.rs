//! Negative tests of the judge (docs/09 9.7 "判定器の負の試験", 9.8 items 3–5): stale verdicts,
//! empty scopes, determinism, case isolation, malformed and oversized input, path escapes and
//! subjects that print text imitating a verdict.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tempdir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!("mukoz-neg-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn mukoz(args: &[&str]) -> (Value, usize) {
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz")).args(args).current_dir(root()).output().unwrap();
    let v = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
    (v, out.stdout.len())
}

fn check(suite: &Path, store: &Path, extra: &[&str]) -> Value {
    let mut a = vec!["check", suite.to_str().unwrap(), "--store", store.to_str().unwrap()];
    a.extend_from_slice(extra);
    mukoz(&a).0
}

fn error_code(v: &Value) -> &str {
    v["errors"][0]["code"].as_str().unwrap_or_else(|| panic!("no error: {v:#}"))
}

fn assessment(v: &Value) -> &Value {
    let a = &v["data"]["assessment"];
    assert!(a.is_object(), "no assessment: {v:#}");
    a
}

/// A private copy of examples/add64 (x86_64) whose files the test may edit.
fn add64_copy() -> PathBuf {
    let d = tempdir("add64");
    for f in ["contract.toml", "binding.x86_64.toml"] {
        std::fs::copy(root().join("examples/add64").join(f), d.join(f)).unwrap();
    }
    let suite = std::fs::read_to_string(root().join("examples/add64/suite.x86_64.toml"))
        .unwrap()
        .replace("../../fixtures", root().join("fixtures").to_str().unwrap());
    std::fs::write(d.join("suite.toml"), suite).unwrap();
    d
}

fn edit(path: &Path, from: &str, to: &str) {
    let s = std::fs::read_to_string(path).unwrap();
    assert!(s.contains(from), "{from} not in {}", path.display());
    std::fs::write(path, s.replacen(from, to, 1)).unwrap();
}

fn fx(n: &str) -> String {
    root().join("fixtures/x86_64").join(format!("{n}.bin")).display().to_string()
}

#[test]
fn a_changed_artifact_contract_binding_or_suite_never_reuses_the_old_verdict() {
    let d = add64_copy();
    let suite = d.join("suite.toml");
    let store = tempdir("store");
    let ctx = |v: &Value| assessment(v)["subject_context"].as_str().unwrap().to_string();

    let v = check(&suite, &store, &["--artifact", &fx("add64_mut_sub")]);
    assert_eq!(assessment(&v)["admission"], "REJECT");
    let c_mut = ctx(&v);
    // Same names, other artifact bytes: judged afresh (and the stored regression cases run).
    let v = check(&suite, &store, &["--artifact", &fx("add64")]);
    assert_eq!(assessment(&v)["admission"], "ACCEPT_WITHIN_SCOPE");
    assert!(assessment(&v)["scope"]["regression_cases_run"].as_u64().unwrap() >= 1);
    let c_ok = ctx(&v);
    assert_ne!(c_mut, c_ok);
    // The mutant again, through a file of the same name as the correct one.
    let same_name = d.join("add64.bin");
    std::fs::copy(fx("add64_mut_sub"), &same_name).unwrap();
    let v = check(&suite, &store, &["--artifact", same_name.to_str().unwrap()]);
    assert_eq!(assessment(&v)["admission"], "REJECT");

    // A comment does not change the contract's meaning or digest.
    edit(&d.join("contract.toml"), "[effects]", "# a comment\n[effects]");
    assert_eq!(ctx(&check(&suite, &store, &["--artifact", &fx("add64")])), c_ok);
    // A changed contract: new context, the old regression cases no longer apply and say so.
    edit(&d.join("contract.toml"), "[effects]", "[[ensures]]\nid = \"extra\"\nexpr = \"true\"\n\n[effects]");
    let v = check(&suite, &store, &["--artifact", &fx("add64")]);
    let c_contract = ctx(&v);
    assert_ne!(c_contract, c_ok);
    assert!(assessment(&v)["scope"]["plan_stats"]["regression_not_applicable"].as_u64().unwrap() >= 1, "{:#}", assessment(&v)["scope"]);
    assert_eq!(assessment(&v)["scope"]["regression_cases_run"], 0);
    // A changed binding, then a changed suite.
    edit(&d.join("binding.x86_64.toml"), "[completion]", "[stack]\nbytes = 32768\n\n[completion]");
    let c_binding = ctx(&check(&suite, &store, &["--artifact", &fx("add64")]));
    assert_ne!(c_binding, c_contract);
    edit(&suite, "random_cases = 4096", "random_cases = 4095");
    let c_suite = ctx(&check(&suite, &store, &["--artifact", &fx("add64")]));
    assert_ne!(c_suite, c_binding);
}

#[test]
fn an_empty_scope_is_hold_never_accept() {
    let d = add64_copy();
    let suite = d.join("suite.toml");
    edit(&suite, "boundary = \"product\"", "boundary = \"none\"");
    edit(&suite, "random_cases = 4096", "random_cases = 0");
    let v = check(&suite, &tempdir("store"), &["--artifact", &fx("add64")]);
    let a = assessment(&v);
    assert_eq!(a["scope"]["cases_completed"], 0);
    assert_eq!(a["admission"], "HOLD");
    assert!(a["reasons"][0].as_str().unwrap().starts_with("VACUOUS_SCOPE"), "{a:#}");
}

/// Same subject context, same platform: the same verdict, claims and counterexample inputs; only
/// the run and counterexample ids differ. Each counterexample replayed alone (a fresh process and
/// engine) observes what it observed inside the batch, so no state leaked into it from earlier
/// cases. This covers the counterexample cases, not every case of the run.
#[test]
fn reruns_and_isolated_replays_agree_with_the_batch() {
    let suite = root().join("examples/add64/suite.x86_64.toml");
    let runs: Vec<Value> = (0..2).map(|_| check(&suite, &tempdir("store"), &["--artifact", &fx("add64_mut_32bit")])).collect();
    let strip = |v: &Value| {
        let d = &v["data"];
        let findings: Vec<_> = d["findings"].as_array().unwrap().iter().map(|f| (f["property"].clone(), f["inputs"].clone(), f["observed"].clone(), f["stop"].clone())).collect();
        let mut claims = d["claims"].clone();
        for c in claims.as_array_mut().unwrap() {
            c.as_object_mut().unwrap().remove("counterexamples");
        }
        (d["assessment"]["admission"].clone(), d["assessment"]["subject_context"].clone(), claims, findings)
    };
    assert_eq!(strip(&runs[0]), strip(&runs[1]));
    assert_ne!(runs[0]["data"]["run_id"], runs[1]["data"]["run_id"]);

    let store = tempdir("store");
    let v = check(&suite, &store, &["--artifact", &fx("add64_mut_32bit")]);
    let findings = v["data"]["findings"].as_array().unwrap();
    assert!(!findings.is_empty());
    for f in findings {
        let (r, _) = mukoz(&["replay", f["counterexample_id"].as_str().unwrap(), "--store", store.to_str().unwrap()]);
        assert_eq!(r["data"]["observed"], f["observed"], "{}", f["counterexample_id"]);
        assert_eq!(r["data"]["stop"], f["stop"]);
        assert_eq!(r["data"]["property_now"], "VIOLATED");
    }
}

#[test]
fn unknown_fields_and_oversized_inputs_are_errors_not_verdicts() {
    let d = add64_copy();
    let store = tempdir("store");
    let suite = d.join("suite.toml");
    std::fs::copy(&suite, d.join("orig.toml")).unwrap();

    edit(&suite, "[limits]", "[limits]\nbogus = 1");
    assert_eq!(error_code(&check(&suite, &store, &[])), "INPUT_ERROR");
    std::fs::copy(d.join("orig.toml"), &suite).unwrap();

    let c = d.join("contract.toml");
    let text = std::fs::read_to_string(&c).unwrap();
    std::fs::write(&c, format!("{text}\n[extra]\nx = 1\n")).unwrap();
    assert_eq!(error_code(&check(&suite, &store, &[])), "INPUT_ERROR");
    std::fs::write(&c, text).unwrap();

    // Refused before any case is generated (it once exhausted memory first).
    edit(&suite, "random_cases = 4096", "random_cases = 100000000");
    let t = std::time::Instant::now();
    assert_eq!(error_code(&check(&suite, &store, &[])), "PLAN_LIMIT_EXCEEDED");
    assert!(t.elapsed().as_secs() < 10, "{:?}", t.elapsed());
    std::fs::copy(d.join("orig.toml"), &suite).unwrap();

    let huge = d.join("huge.bin");
    let f = std::fs::File::create(&huge).unwrap();
    f.set_len(65 << 20).unwrap();
    let v = check(&suite, &store, &["--artifact", huge.to_str().unwrap()]);
    assert_eq!(error_code(&v), "INPUT_ERROR");
    assert!(v["errors"][0]["message"].as_str().unwrap().contains("limit"), "{v:#}");
}

#[test]
fn binding_file_paths_cannot_leave_the_subject_file_system() {
    let d = tempdir("todo");
    for e in std::fs::read_dir(root().join("examples/todo")).unwrap() {
        let p = e.unwrap().path();
        std::fs::copy(&p, d.join(p.file_name().unwrap())).unwrap();
    }
    let suite = d.join("suite.x86_64.elf.toml");
    let art = root().join("fixtures/process/todo_x86.elf");
    let binding = std::fs::read_to_string(&suite).unwrap().lines().find(|l| l.starts_with("binding")).unwrap().split('"').nth(1).unwrap().to_string();
    for bad in ["../todo.db", "/../etc/passwd", "a/./b", "a//b"] {
        let b = d.join(&binding);
        let orig = std::fs::read_to_string(&b).unwrap();
        edit(&b, "path = \"todo.db\"", &format!("path = \"{bad}\""));
        let v = check(&suite, &tempdir("store"), &["--artifact", art.to_str().unwrap()]);
        assert_eq!(error_code(&v), "BINDING_MISMATCH", "{bad}: {v:#}");
        std::fs::write(&b, orig).unwrap();
    }
}

/// The subject's stdout imitates a Mukoz verdict (once, or forever): it is data in the
/// observation, never Mukoz's output, and Mukoz's own output stays small.
#[test]
fn a_subject_printing_a_verdict_cannot_fake_one() {
    let pol = tempdir("pol").join("policy.toml");
    std::fs::write(
        &pol,
        format!(
            "[[native_trial_zones]]\nid = \"fx\"\nartifact_dir = \"{}\"\nexecutors = [\"native-process\"]\ntargets = [\"x86_64/elf/sysv-x86_64/linux\"]\n\
             require_isolation = [\"network_restriction\", \"filesystem_restriction\", \"resource_limits\", \"descendant_process_control\", \"credential_isolation\"]\nmax_wall_ms_per_case = 2000\n",
            root().join("fixtures/process").display()
        ),
    )
    .unwrap();
    let mut suites = vec!["examples/hello/suite.x86_64.elf.toml"];
    let native = Command::new(env!("CARGO_BIN_EXE_mukoz")).args(["platform", "probe", "--store", tempdir("probe").to_str().unwrap()]).output().unwrap();
    let probe: Value = serde_json::from_slice(&native.stdout).unwrap();
    if probe.to_string().contains("\"seccomp_strict\"") && std::env::consts::ARCH == "x86_64" {
        suites.push("examples/hello/suite.x86_64.elf.native.toml");
    }
    for f in ["fake_accept_x86", "fake_accept_x86_flood"] {
        for s in &suites {
            let art = root().join("fixtures/process").join(format!("{f}.elf"));
            let (v, len) = mukoz(&["check", s, "--artifact", art.to_str().unwrap(), "--store", tempdir("store").to_str().unwrap(), "--policy", pol.to_str().unwrap()]);
            assert_eq!(assessment(&v)["admission"], "REJECT", "{f} {s}: {:#}", assessment(&v));
            assert!(len < 64 << 10, "{f} {s}: mukoz printed {len} bytes");
        }
    }
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { files_under(&p, out) } else { out.push(p) }
    }
}

/// docs/04 4.8: the regression set is never thinned silently. A full set keeps the verdict and
/// counts the new cases it could not keep; a set over the limit refuses the plan.
#[test]
fn the_regression_limit_never_drops_cases_or_verdicts_silently() {
    let d = add64_copy();
    let suite = d.join("suite.toml");
    let store = tempdir("store");
    assert_eq!(assessment(&check(&suite, &store, &["--artifact", &fx("add64_mut_sub")]))["admission"], "REJECT");
    let mut files = Vec::new();
    files_under(&store.join("regressions"), &mut files);
    let files: Vec<_> = files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    let dir = files[0].parent().unwrap().to_path_buf();
    let have = std::fs::read_dir(&dir).unwrap().count();
    for i in have..1024 {
        std::fs::copy(&files[0], dir.join(format!("filler{i:04}.json"))).unwrap();
    }
    let v = check(&suite, &store, &["--artifact", &fx("add64_mut_32bit")]);
    let a = assessment(&v);
    assert_eq!(a["admission"], "REJECT", "{a:#}");
    assert!(a["limitations"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().starts_with("regression_limit_reached_")), "{a:#}");
    std::fs::copy(&files[0], dir.join("filler_over.json")).unwrap();
    assert_eq!(error_code(&check(&suite, &store, &["--artifact", &fx("add64")])), "REGRESSION_LIMIT_EXCEEDED");
    // Excluding the regression set is allowed, and reported.
    let text = std::fs::read_to_string(&suite).unwrap();
    std::fs::write(&suite, format!("{text}\n[regressions]\ninclude = false\n")).unwrap();
    let v = check(&suite, &store, &["--artifact", &fx("add64")]);
    assert_eq!(assessment(&v)["admission"], "ACCEPT_WITHIN_SCOPE");
    assert!(assessment(&v)["limitations"].as_array().unwrap().iter().any(|l| l == "regression_cases_excluded"));
}

/// A missing or half-written evidence item is an error, never a replay that passes.
#[test]
fn missing_or_damaged_evidence_is_an_error() {
    let store = tempdir("store");
    let v = check(&root().join("examples/add64/suite.x86_64.toml"), &store, &["--artifact", &fx("add64_mut_sub")]);
    let ids: Vec<String> = v["data"]["claims"].as_array().unwrap().iter().flat_map(|c| c["counterexamples"].as_array().cloned().unwrap_or_default()).map(|x| x.as_str().unwrap().to_string()).collect();
    assert!(ids.len() >= 2);
    let item = |id: &str| store.join("items").join(format!("{id}.json"));
    std::fs::remove_file(item(&ids[0])).unwrap();
    let text = std::fs::read(item(&ids[1])).unwrap();
    std::fs::write(item(&ids[1]), &text[..text.len() / 2]).unwrap();
    for id in &ids[..2] {
        for cmd in ["replay", "show", "shrink"] {
            let (r, _) = mukoz(&[cmd, id, "--store", store.to_str().unwrap()]);
            assert_eq!(r["ok"], false, "{cmd} {id}: {r:#}");
            assert!(r["data"].is_null(), "{cmd} {id}");
        }
    }
}

/// A binding that misplaces the entry, the arguments or the result, or names a narrower result
/// register, never yields ACCEPT for a correct add64.
#[test]
fn a_misplaced_binding_is_never_accepted() {
    for (from, to, want) in [
        ("offset = 0", "offset = 1", "REJECT"),
        ("rdi = \"input.a\"", "rdx = \"input.a\"", "REJECT"),
        ("value = \"rax\"", "value = \"rdx\"", "REJECT"),
        ("value = \"rax\"", "value = \"eax\"", "BINDING_MISMATCH"),
    ] {
        let d = add64_copy();
        edit(&d.join("binding.x86_64.toml"), from, to);
        let v = check(&d.join("suite.toml"), &tempdir("store"), &[]);
        let got = if v["data"].is_null() { error_code(&v).to_string() } else { assessment(&v)["admission"].as_str().unwrap().to_string() };
        assert_eq!(got, want, "{from} -> {to}");
    }
}
