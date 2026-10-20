//! The four security example families must keep their announced verdicts and replayable blame.
use std::process::Command;

#[test]
fn security_example_matrix_and_counterexamples_remain_reproducible() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let out = std::env::temp_dir().join(format!(
        "mukoz-security-matrix-{}-{unique}",
        std::process::id()
    ));
    let result = Command::new("python3")
        .arg("examples/security_checks/run.py")
        .arg(&out)
        .args(["--mukoz", env!("CARGO_BIN_EXE_mukoz")])
        .output()
        .expect("run security example matrix");
    assert!(
        result.status.success(),
        "records: {}\n{}\n{}",
        out.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("summary.json")).unwrap()).unwrap();
    assert_eq!(report["checks"], 30);
    assert_eq!(report["replays"], 16);
    assert_eq!(report["problems"].as_array().unwrap().len(), 0);
    std::fs::remove_dir_all(out).unwrap();
}
