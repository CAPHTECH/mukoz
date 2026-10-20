//! Access contracts add a check that final-state equality and allocator addressability cannot give.
//! The identical routines are exercised with broad/strict access and snapshot comparison by X3.
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
fn temp() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "mukoz-security-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn check(isa: &str, name: &str, suite: Option<&Path>) -> Value {
    let tmp = temp();
    let suite_path = format!("examples/security_access/suite.{isa}.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_mukoz"))
        .arg("check")
        .arg(suite.unwrap_or_else(|| Path::new(&suite_path)))
        .args([
            "--artifact",
            &format!("examples/security_access/{isa}/{name}.bin"),
            "--store",
        ])
        .arg(&tmp)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    std::fs::remove_dir_all(tmp).unwrap();
    assert_eq!(value["ok"], true, "{value}");
    value["data"].clone()
}

#[test]
fn caller_access_permissions_reject_hidden_reads_and_restored_writes() {
    for isa in ["x86_64", "aarch64"] {
        for (name, access) in [
            ("read_audit", "read"),
            ("restore_source", "write"),
            ("read_past_source", "read"),
        ] {
            let data = check(isa, name, None);
            assert_eq!(data["assessment"]["admission"], "REJECT", "{isa}/{name}");
            let findings = data["findings"].as_array().unwrap();
            assert!(
                !findings.is_empty(),
                "{isa}/{name}: must expose a replayable access"
            );
            let memory = data["claims"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["property"] == "machine.memory.access")
                .unwrap();
            assert_eq!(
                memory["cases_violated"], 24,
                "{isa}/{name}: every planned case must expose the access"
            );
            for f in findings {
                assert_eq!(f["property"], "machine.memory.access");
                assert_eq!(f["stop"]["access"], access);
                assert_eq!(f["stop"]["size"], 1);
                assert!(f["counterexample_id"].as_str().unwrap().starts_with("cx-"));
            }
        }
    }
}

#[test]
fn correct_copy_and_permitted_stack_scratch_are_accepted() {
    for isa in ["x86_64", "aarch64"] {
        for name in ["correct", "scratch"] {
            let data = check(isa, name, None);
            assert_eq!(
                data["assessment"]["admission"], "ACCEPT_WITHIN_SCOPE",
                "{isa}/{name}"
            );
            assert_eq!(data["assessment"]["scope"]["cases_completed"], 24);
        }
    }
}

#[test]
fn the_same_binary_obeys_the_callers_permissions_rather_than_a_bad_byte_pattern() {
    for (name, from) in [
        ("read_audit", "access = \"w\""),
        ("restore_source", "access = \"r\""),
    ] {
        let dir = temp();
        let root = Path::new("examples/security_access");
        for file in ["contract.toml", "suite.x86_64.toml"] {
            std::fs::copy(root.join(file), dir.join(file)).unwrap();
        }
        let binding = std::fs::read_to_string(root.join("binding.x86_64.toml")).unwrap();
        std::fs::write(
            dir.join("binding.x86_64.toml"),
            binding.replace(from, "access = \"rw\""),
        )
        .unwrap();
        let data = check("x86_64", name, Some(&dir.join("suite.x86_64.toml")));
        assert_eq!(
            data["assessment"]["admission"], "ACCEPT_WITHIN_SCOPE",
            "{name}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
