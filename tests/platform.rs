//! Host probe and engine qualification (docs/02 2.5, docs/03 3.6 rule 4, docs/09 9.7 Platform).

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn tempdir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!("mukoz-platform-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn mukoz(args: &[&str]) -> (Value, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz")).args(args).current_dir(env!("CARGO_MANIFEST_DIR")).output().expect("run mukoz");
    let v = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("bad json: {e}\n{}", String::from_utf8_lossy(&out.stdout)));
    (v, out.status.code().unwrap_or(-1))
}

#[test]
fn both_isas_qualify_with_known_answers() {
    let st = tempdir("q");
    for isa in ["x86_64", "aarch64"] {
        let (v, code) = mukoz(&["platform", "qualify", "--isa", isa, "--store", st.to_str().unwrap()]);
        assert_eq!(code, 0, "{v:#}");
        assert_eq!(v["data"]["passed"], true);
        assert!(v["data"]["vectors"].as_u64().unwrap() > 300);
        assert!(st.join(format!("host/qualification-{isa}.json")).exists());
    }
    let (v, _) = mukoz(&["platform", "show", "--store", st.to_str().unwrap()]);
    assert_eq!(v["data"]["engine_qualification"]["aarch64"]["passed"], true);
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        let (v, _) = mukoz(&["platform", "qualify", "--isa", "x86_64", "--store", st.to_str().unwrap()]);
        assert_eq!(v["data"]["native_cross_check"]["passed"], true, "{:#}", v["data"]["native_cross_check"]);
        // Unicorn's default CPU model has no POPCNT; other engines may implement it.
        if v["data"]["engine"].as_str().unwrap_or("").starts_with("unicorn") {
            assert_eq!(v["data"]["unsupported_by_engine"][0], "popcnt");
        }
    }
}

#[test]
fn a_failed_qualification_holds_every_emulated_result_and_is_not_rerolled() {
    let st = tempdir("fail");
    let s = st.to_str().unwrap();
    let (v, _) = mukoz(&["check", "examples/add64/suite.x86_64.toml", "--store", s]);
    assert_eq!(v["data"]["assessment"]["admission"], "ACCEPT_WITHIN_SCOPE");
    assert_eq!(v["data"]["assessment"]["scope"]["engine_qualification"]["run_during_this_check"], true);
    let p = st.join("host/qualification-x86_64.json");
    let mut q: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    q["passed"] = Value::Bool(false);
    q["failures"] = serde_json::json!([{ "test": "injected" }]);
    std::fs::write(&p, serde_json::to_vec(&q).unwrap()).unwrap();
    for art in ["fixtures/x86_64/add64.bin", "fixtures/x86_64/add64_mut_sub.bin"] {
        let (v, code) = mukoz(&["check", "examples/add64/suite.x86_64.toml", "--artifact", art, "--store", s, "--gate"]);
        let a = &v["data"]["assessment"];
        assert_eq!(a["admission"], "HOLD", "{art}");
        assert_eq!(code, 10);
        assert!(a["reasons"][0].as_str().unwrap().starts_with("ENGINE_NOT_QUALIFIED"));
        assert_eq!(a["scope"]["engine_qualification"]["run_during_this_check"], false);
    }
    // A record for another host is not used: the check qualifies again.
    q["host_id"] = Value::String("other-host".into());
    std::fs::write(&p, serde_json::to_vec(&q).unwrap()).unwrap();
    let (v, _) = mukoz(&["check", "examples/add64/suite.x86_64.toml", "--store", s]);
    assert_eq!(v["data"]["assessment"]["admission"], "ACCEPT_WITHIN_SCOPE");
    assert_eq!(v["data"]["assessment"]["scope"]["engine_qualification"]["run_during_this_check"], true);
}

#[test]
#[cfg(target_os = "linux")]
fn probe_confirms_capabilities_only_with_a_control_run() {
    let st = tempdir("probe");
    let (v, _) = mukoz(&["platform", "probe", "--store", st.to_str().unwrap()]);
    let caps = &v["data"]["capabilities"];
    for k in ["network_restriction", "filesystem_restriction", "resource_limits", "descendant_process_control", "seccomp_strict"] {
        let c = &caps[k];
        // A confirmed capability must have blocked the access and the control must have reached it.
        if c["confirmed"] == true {
            assert_eq!(c["restricted_blocked"], true, "{k}");
            assert_eq!(c["control_reached"], true, "{k}");
        }
    }
}
