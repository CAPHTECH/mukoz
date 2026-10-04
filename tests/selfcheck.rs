//! Self-check stage 3 and the independence record (docs/09 9.6, docs/06 6.9).

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn store() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("mukoz-self-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))).display().to_string()
}

fn mukoz(args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz")).args(args).current_dir(root()).output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

fn suites() -> Vec<String> {
    let mut v = Vec::new();
    for g in ["bv", "range", "elfhdr", "copy"] {
        for e in std::fs::read_dir(root().join("selfcheck/stage3").join(g)).unwrap() {
            let p = e.unwrap().path();
            if p.file_name().unwrap().to_str().unwrap().starts_with("suite.") {
                v.push(p.strip_prefix(root()).unwrap().display().to_string());
            }
        }
    }
    v.sort();
    v
}

#[test]
fn stage3_kernels_hold_only_because_the_checker_is_self() {
    let pol = "selfcheck/stage3/policy.toml";
    let mut n = 0;
    for s in suites() {
        let v = mukoz(&["check", &s, "--store", &store(), "--policy", pol]);
        if s.contains("/copy/") {
            assert_eq!(v["errors"][0]["code"], "UNRESOLVED_DEPENDENCY", "{s}: memcpy must not be judged");
            continue;
        }
        let a = &v["data"]["assessment"];
        assert_eq!(a["independence"]["value"], "self", "{s}");
        assert_eq!(a["admission"], "HOLD", "{s}");
        let reasons = a["reasons"].as_array().unwrap();
        assert!(reasons.len() == 1 && reasons[0].as_str().unwrap().starts_with("SELF_CHECK_ONLY"), "{s}: {reasons:?}");
        for c in v["data"]["claims"].as_array().unwrap() {
            assert_eq!(c["evaluation"], "SATISFIED_IN_SCOPE", "{s}: {c}");
        }
        n += 1;
    }
    assert_eq!(n, 14, "5 bv kernels + range + elf header, on 2 ISAs");
}

#[test]
fn a_listed_previous_version_is_recorded_and_can_accept() {
    // Simulate N-1: list the running binary's digest as the previous version.
    use sha2::Digest;
    let exe = std::fs::read(env!("CARGO_BIN_EXE_mukoz")).unwrap();
    let digest = format!("{:x}", sha2::Sha256::digest(&exe));
    let dir = std::env::temp_dir().join(format!("mukoz-prev-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("checkers.toml"), format!("schema = \"mukoz.checkers/1\"\nprevious = [{{ version = \"test-n-1\", sha256 = \"{digest}\" }}]\n")).unwrap();
    let suite = std::fs::read_to_string(root().join("selfcheck/stage3/range/suite.aarch64.toml")).unwrap();
    let suite = suite
        .replace("contract = \"contract.toml\"", &format!("contract = \"{}\"", root().join("selfcheck/stage3/range/contract.toml").display()))
        .replace("binding = \"binding.aarch64.toml\"", &format!("binding = \"{}\"", root().join("selfcheck/stage3/range/binding.aarch64.toml").display()))
        .replace("path = \"../obj/kernels_aarch64.o\"", &format!("path = \"{}\"", root().join("selfcheck/stage3/obj/kernels_aarch64.o").display()))
        .replace("checkers = \"../../checkers.toml\"", "checkers = \"checkers.toml\"");
    std::fs::write(dir.join("suite.toml"), suite).unwrap();
    let v = mukoz(&["check", dir.join("suite.toml").to_str().unwrap(), "--store", &store()]);
    let a = &v["data"]["assessment"];
    assert_eq!(a["independence"]["value"], "previous_version");
    assert_eq!(a["independence"]["checker_version"], "test-n-1");
    assert_eq!(a["admission"], "ACCEPT_WITHIN_SCOPE");
    // Ordinary subjects are independent of Mukoz.
    let v = mukoz(&["check", "examples/add64/suite.x86_64.toml", "--store", &store()]);
    assert_eq!(v["data"]["assessment"]["independence"]["value"], "independent");
}
