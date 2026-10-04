//! P4: navigation from a run summary to a finding, its counterexample and the trace; paging;
//! disassembly; shrink; replay (docs/07 7.5, docs/09 9.8 items 2 and 7).

use serde_json::Value;
use std::process::Command;

fn mukoz(args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz")).args(args).current_dir(env!("CARGO_MANIFEST_DIR")).output().expect("run mukoz");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("bad json: {e}\n{}", String::from_utf8_lossy(&out.stdout)))
}

fn store() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("mukoz-nav-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))).display().to_string()
}

#[test]
fn summary_to_finding_to_counterexample_to_trace() {
    let st = store();
    let v = mukoz(&["check", "examples/add64/suite.x86_64.toml", "--artifact", "fixtures/x86_64/add64_mut_sub.bin", "--store", &st]);
    let d = &v["data"];
    // 1. summary: admission and the violated property
    assert_eq!(d["assessment"]["admission"], "REJECT");
    let run_id = d["run_id"].as_str().unwrap();
    let f = &d["findings"][0];
    assert_eq!(f["property"], "arith.add64/sum");
    // 2. the run can be reopened by id
    let r = mukoz(&["show", run_id, "--store", &st]);
    assert_eq!(r["data"]["kind"], "run");
    // 3. counterexample: inputs, expected/observed, why the ensures is false
    let cx = f["counterexample_id"].as_str().unwrap();
    let c = mukoz(&["show", cx, "--store", &st]);
    assert_eq!(c["data"]["kind"], "counterexample");
    assert!(c["data"]["detail"]["why_false"].is_array());
    // 4. trace with disassembly
    let t = mukoz(&["show", cx, "--disasm", "--store", &st]);
    let insns = t["data"]["recent_instructions"].as_array().unwrap();
    assert!(insns.iter().any(|i| i["asm"].as_str().unwrap().starts_with("sub rax")), "{insns:?}");
    // 5. replay reproduces
    let p = mukoz(&["replay", cx, "--store", &st]);
    assert_eq!(p["data"]["property_now"], "VIOLATED");
    assert_eq!(p["data"]["same_artifact_contract_binding"], true);
    // and the fixed artifact no longer reproduces it
    let p = mukoz(&["replay", cx, "--artifact", "fixtures/x86_64/add64.bin", "--store", &st]);
    assert_eq!(p["data"]["property_now"], "SATISFIED_IN_SCOPE");
    assert_eq!(p["data"]["same_artifact_contract_binding"], false);
}

#[test]
fn long_lists_are_paged() {
    let st = store();
    let v = mukoz(&["check", "examples/todo/suite.x86_64.elf.toml", "--artifact", "fixtures/process/todo_x86_mut_nonl.elf", "--store", &st]);
    let cx = v["data"]["findings"][0]["counterexample_id"].as_str().unwrap().to_string();
    let p1 = mukoz(&["show", &cx, "--store", &st]);
    let insns = p1["data"]["recent_instructions"].as_array().unwrap();
    assert_eq!(insns.len(), 32);
    let lists = p1["data"]["_pages"]["lists"].as_array().unwrap();
    let ri = lists.iter().find(|l| l["path"] == "/recent_instructions").unwrap();
    assert_eq!(ri["pages"], 2);
    let p2 = mukoz(&["show", &cx, "--page", "2", "--store", &st]);
    assert_ne!(p2["data"]["recent_instructions"][0], insns[0]);
}

#[test]
fn shrink_keeps_the_property_and_requires_and_stores_a_replayable_case() {
    let st = store();
    let v = mukoz(&["check", "examples/todo/suite.x86_64.elf.toml", "--artifact", "fixtures/process/todo_x86_mut_nonl.elf", "--store", &st]);
    let claim = v["data"]["claims"].as_array().unwrap().iter().find(|c| c["property"] == "proc.todo/add_appends_line").unwrap();
    for cx in claim["counterexamples"].as_array().unwrap() {
        let s = mukoz(&["shrink", cx.as_str().unwrap(), "--store", &st]);
        let d = &s["data"];
        assert_eq!(d["completed"], true, "{d:#}");
        assert_eq!(d["property"], "proc.todo/add_appends_line");
        let text = d["inputs"]["input.text"]["len"].as_u64().unwrap();
        assert!(text <= 1, "{d:#}");
        let r = mukoz(&["replay", d["counterexample_id"].as_str().unwrap(), "--store", &st]);
        assert_eq!(r["data"]["property_now"], "VIOLATED");
    }
    // A counterexample that does not reproduce is refused, not "shrunk".
    let r = mukoz(&["shrink", "cx-000000000000", "--store", &st]);
    assert_eq!(r["ok"], false);
}
