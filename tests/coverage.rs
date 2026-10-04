//! docs/09 9.7 fixture coverage on both ISAs: each row's admission and property were written
//! before running it (signed/unsigned comparison, checked increment, bounded copy, nested call,
//! callee-saved, x18 on aapcs64 vs apple-arm64, infinite loop, unsupported instruction).

use serde_json::Value;
use std::process::Command;

fn check(suite: &str, art: &str) -> Value {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let st = std::env::temp_dir().join(format!("mukoz-cov-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz"))
        .args(["check", suite, "--artifact", &format!("fixtures/{art}.bin"), "--store", st.to_str().unwrap()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

const ROWS: &[(&str, &str, &str, &str)] = &[
    ("examples/cmp/suite.x86_64.toml", "x86_64/cmp", "ACCEPT_WITHIN_SCOPE", ""),
    ("examples/cmp/suite.x86_64.toml", "x86_64/cmp_mut_signed_as_unsigned", "REJECT", "cmp.less/signed"),
    ("examples/cmp/suite.aarch64.toml", "aarch64/cmp", "ACCEPT_WITHIN_SCOPE", ""),
    ("examples/cmp/suite.aarch64.toml", "aarch64/cmp_mut_signed_as_unsigned", "REJECT", "cmp.less/signed"),
    ("examples/add64/suite.aarch64.toml", "aarch64/add64_nested", "ACCEPT_WITHIN_SCOPE", ""),
    ("examples/add64/suite.aarch64.toml", "aarch64/add64_mut_clobber_x19", "REJECT", "machine.abi.callee_saved"),
    ("examples/add64/suite.aarch64.toml", "aarch64/add64_x18_temp", "ACCEPT_WITHIN_SCOPE", ""),
    ("examples/add64/suite.aarch64_apple.toml", "aarch64/add64_x18_temp", "REJECT", "machine.abi.reserved"),
    ("examples/add64/suite.aarch64_apple.toml", "aarch64/add64", "ACCEPT_WITHIN_SCOPE", ""),
    ("examples/add64/suite.aarch64.toml", "aarch64/add64_mut_loop", "HOLD", "BUDGET_EXHAUSTED"),
    ("examples/add64/suite.aarch64.toml", "aarch64/add64_mut_udf", "HOLD", "UNSUPPORTED_DURING_RUN"),
    ("examples/add64/suite.aarch64.toml", "aarch64/add64_mut_brk", "REJECT", "effects.no_forbidden"),
    ("examples/checked_inc/suite.aarch64.toml", "aarch64/checked_inc", "ACCEPT_WITHIN_SCOPE", ""),
    ("examples/checked_inc/suite.aarch64.toml", "aarch64/checked_inc_mut_nocheck", "REJECT", "counter.checked_inc/ok_iff_no_overflow"),
    ("examples/copy/suite.aarch64.toml", "aarch64/copy", "ACCEPT_WITHIN_SCOPE", ""),
    ("examples/copy/suite.aarch64.toml", "aarch64/copy_mut_offbyone", "REJECT", "machine.memory.access"),
];

#[test]
fn fixture_rows_match_their_announced_outcome() {
    let mut bad = Vec::new();
    for (suite, art, want, what) in ROWS {
        let v = check(suite, art);
        let a = &v["data"]["assessment"];
        let viol: Vec<&str> = v["data"]["claims"].as_array().unwrap().iter().filter(|c| c["evaluation"] == "VIOLATED").map(|c| c["property"].as_str().unwrap()).collect();
        let reasons = a["reasons"].to_string();
        let ok = a["admission"] == *want && (what.is_empty() || viol.contains(what) || reasons.contains(what));
        if *want == "REJECT" && ok {
            // Rejections name exactly the announced property among the violated ones.
            assert!(viol.contains(what), "{art}: {viol:?}");
        }
        if !ok {
            bad.push(format!("{art} via {suite}: {} {viol:?} {reasons}", a["admission"]));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
