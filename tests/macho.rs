//! Mach-O loading and darwin-stdio/1 (docs/09 9.4 P3, 9.7: hello Mach-O, broken Mach-O).

use serde_json::Value;
use std::process::Command;

fn mukoz(args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mukoz")).args(args).current_dir(env!("CARGO_MANIFEST_DIR")).output().expect("run mukoz");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("bad json: {e}\n{}", String::from_utf8_lossy(&out.stdout)))
}

fn store() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("mukoz-macho-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))).display().to_string()
}

fn check(suite: &str, art: &str) -> Value {
    mukoz(&["check", suite, "--artifact", &format!("fixtures/process/{art}"), "--store", &store()])
}

const S: &str = "examples/hello/suite.aarch64.macho.toml";

#[test]
fn macho_hello_is_checked_on_the_emulator() {
    for a in ["hello_a64.macho", "hello_a64_eq.macho"] {
        let v = check(S, a);
        assert_eq!(v["data"]["assessment"]["admission"], "ACCEPT_WITHIN_SCOPE", "{a}: {:#}", v["data"]["assessment"]);
    }
    let v = check(S, "hello_a64_mut_len.macho");
    assert_eq!(v["data"]["assessment"]["admission"], "REJECT");
    assert_eq!(v["data"]["findings"][0]["property"], "proc.hello/greets");
    let v = check(S, "hello_a64_mut_x18.macho");
    assert_eq!(v["data"]["assessment"]["admission"], "REJECT");
    assert!(v["data"]["assessment"]["reasons"].as_array().unwrap().iter().any(|r| r == "VIOLATED: machine.abi.reserved"), "{:#}", v["data"]["assessment"]);
}

#[test]
fn macho_native_is_host_cannot_execute_target() {
    let v = check("examples/hello/suite.aarch64.macho.diff.toml", "hello_a64.macho");
    assert_eq!(v["data"]["assessment"]["admission"], "HOLD");
    let c = v["data"]["claims"].as_array().unwrap().iter().find(|c| c["property"] == "differential.emulated_vs_native-process").unwrap();
    assert!(c["not_evaluated_reasons"]["HOST_CANNOT_EXECUTE_TARGET"].as_u64().unwrap() > 0, "{c}");
}

#[test]
fn macho_with_a_dylib_is_an_unresolved_dependency() {
    let v = check(S, "hello_a64_dylib.macho");
    assert_eq!(v["errors"][0]["code"], "UNRESOLVED_DEPENDENCY");
}

#[test]
fn broken_macho_headers_are_format_errors() {
    for f in ["broken_truncated", "broken_cmdsize", "broken_fileoff", "broken_ncmds"] {
        let v = mukoz(&["inspect", &format!("fixtures/process/{f}.macho")]);
        assert_eq!(v["data"]["load"]["ok"], false, "{f}");
        assert_eq!(v["data"]["load"]["code"], "FORMAT_MISMATCH", "{f}: {}", v["data"]["load"]);
    }
    for f in ["broken_truncated", "broken_phoff", "broken_filesz"] {
        let v = mukoz(&["inspect", &format!("fixtures/process/{f}.elf")]);
        assert_eq!(v["data"]["load"]["code"], "FORMAT_MISMATCH", "{f}");
    }
    let v = mukoz(&["inspect", "fixtures/process/hello_dyn.elf"]);
    assert_eq!(v["data"]["load"]["code"], "UNRESOLVED_DEPENDENCY");
}
