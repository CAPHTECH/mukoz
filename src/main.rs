//! `mukoz` CLI (docs/07). Every command prints one JSON envelope.

mod emu;
mod expr;
mod judge;
mod plan;
mod run;
mod spec;
mod store;

use judge::Admission;
use serde_json::json;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage:
  mukoz check <suite.toml> [--artifact <file>] [--fail-fast] [--gate] [--store <dir>]
  mukoz show <id> [--store <dir>]
  mukoz replay <counterexample-id> [--artifact <file>] [--store <dir>]
  mukoz inspect <file>
  mukoz regressions list <suite.toml> [--store <dir>]
  mukoz regressions prune <suite.toml> --case <id> [--store <dir>]
  mukoz platform show
  mukoz expr check <expression>          (parse and print the normalized form)";

struct Args {
    pos: Vec<String>,
    flags: Vec<String>,
    opts: Vec<(String, String)>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { pos: Vec::new(), flags: Vec::new(), opts: Vec::new() };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--fail-fast" | "--gate" | "--help" | "-h" => a.flags.push(x),
            "--artifact" | "--store" | "--case" => {
                let v = it.next().ok_or(format!("{x} needs a value"))?;
                a.opts.push((x, v));
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => a.pos.push(x),
        }
    }
    Ok(a)
}

impl Args {
    fn opt(&self, k: &str) -> Option<&str> {
        self.opts.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    fn flag(&self, k: &str) -> bool {
        self.flags.iter().any(|f| f == k)
    }
}

fn error_code(msg: &str) -> String {
    let head = msg.split(':').next().unwrap_or("");
    if !head.is_empty() && head.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
        head.to_string()
    } else {
        "INPUT_ERROR".to_string()
    }
}

fn emit(command: &str, ok: bool, data: serde_json::Value, errors: Vec<serde_json::Value>) {
    let env = json!({ "api_version": "mukoz/1", "command": command, "ok": ok, "data": data, "errors": errors });
    println!("{}", serde_json::to_string_pretty(&env).unwrap());
}

fn fail(command: &str, e: anyhow::Error, code: i32) -> i32 {
    let msg = format!("{e:#}");
    emit(command, false, json!(null), vec![json!({ "code": error_code(&msg), "message": msg })]);
    code
}

fn open_store(a: &Args) -> anyhow::Result<store::Store> {
    let root = a.opt("--store").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".mukoz"));
    store::Store::open(&root)
}

fn real_main() -> i32 {
    let a = match parse_args() {
        Ok(a) => a,
        Err(m) => {
            eprintln!("{m}\n{USAGE}");
            return 2;
        }
    };
    if a.flag("--help") || a.flag("-h") || a.pos.is_empty() {
        eprintln!("{USAGE}");
        return if a.pos.is_empty() && !a.flag("--help") && !a.flag("-h") { 2 } else { 0 };
    }
    let cmd = a.pos[0].as_str();
    match (cmd, a.pos.len()) {
        ("check", 2) => {
            let st = match open_store(&a) {
                Ok(s) => s,
                Err(e) => return fail("check", e, 4),
            };
            let o = run::CheckOpts {
                suite: Path::new(&a.pos[1]),
                artifact: a.opt("--artifact").map(Path::new),
                fail_fast: a.flag("--fail-fast"),
                store: &st,
            };
            match run::check(&o) {
                Ok((data, adm)) => {
                    emit("check", true, data, vec![]);
                    if a.flag("--gate") {
                        match adm {
                            Admission::AcceptWithinScope => 0,
                            Admission::Hold => 10,
                            Admission::Reject => 11,
                        }
                    } else {
                        0
                    }
                }
                Err(e) => fail("check", e, 2),
            }
        }
        ("show", 2) => match open_store(&a).and_then(|s| s.get_item(&a.pos[1])) {
            Ok(v) => {
                emit("show", true, v, vec![]);
                0
            }
            Err(e) => fail("show", e, 2),
        },
        ("replay", 2) => match open_store(&a).and_then(|s| run::replay(&s, &a.pos[1], a.opt("--artifact").map(Path::new))) {
            Ok(v) => {
                emit("replay", true, v, vec![]);
                0
            }
            Err(e) => fail("replay", e, 2),
        },
        ("inspect", 2) => {
            let p = Path::new(&a.pos[1]);
            match std::fs::read(p) {
                Ok(b) => {
                    let format = if b.starts_with(b"\x7fELF") {
                        "elf"
                    } else if b.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]) || b.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) {
                        "macho"
                    } else if b.starts_with(b"MZ") {
                        "pe"
                    } else {
                        "raw"
                    };
                    emit(
                        "inspect",
                        true,
                        json!({
                            "path": p.display().to_string(),
                            "bytes": b.len(),
                            "sha256": spec::sha256_hex(&b),
                            "format_guess": format,
                            "executable_by_mukoz": if format == "raw" { "as a raw routine via a binding" } else { "UNSUPPORTED_FEATURE: only raw routines are implemented" },
                            "head_hex": expr::hex(&b[..b.len().min(64)]),
                        }),
                        vec![],
                    );
                    0
                }
                Err(e) => fail("inspect", e.into(), 2),
            }
        }
        ("regressions", 3) if a.pos[1] == "list" => {
            let r = (|| -> anyhow::Result<serde_json::Value> {
                let st = open_store(&a)?;
                let suite = spec::Suite::load(Path::new(&a.pos[2]))?;
                let c = spec::Contract::load(&suite.contract_path)?;
                let b = spec::Binding::load(&suite.binding_path, &c, &emu::is_arg_or_result_reg)?;
                let items = st.list_regressions(&c, &b.target.text)?;
                Ok(json!({ "contract": c.id, "target": b.target.text, "total": items.len(), "items": items }))
            })();
            match r {
                Ok(v) => {
                    emit("regressions list", true, v, vec![]);
                    0
                }
                Err(e) => fail("regressions list", e, 2),
            }
        }
        ("regressions", 3) if a.pos[1] == "prune" => {
            let r = (|| -> anyhow::Result<serde_json::Value> {
                let id = a.opt("--case").ok_or_else(|| anyhow::anyhow!("USAGE: --case <id> is required"))?;
                let st = open_store(&a)?;
                let suite = spec::Suite::load(Path::new(&a.pos[2]))?;
                let c = spec::Contract::load(&suite.contract_path)?;
                let b = spec::Binding::load(&suite.binding_path, &c, &emu::is_arg_or_result_reg)?;
                st.prune_regression(&c, &b.target.text, id)?;
                Ok(json!({ "pruned": id, "logged_to": "regressions/prune-log.jsonl" }))
            })();
            match r {
                Ok(v) => {
                    emit("regressions prune", true, v, vec![]);
                    0
                }
                Err(e) => fail("regressions prune", e, 2),
            }
        }
        ("platform", 2) if a.pos[1] == "show" => {
            emit(
                "platform show",
                true,
                json!({
                    "host": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                    "mukoz": run::EVALUATOR_VERSION,
                    "executors": {
                        "emulated": { "available": true, "engine": emu::ENGINE, "isas": ["x86_64", "aarch64"] },
                        "native-routine": { "available": false, "reason": "not implemented" },
                        "native-process": { "available": false, "reason": "not implemented" },
                        "translated-process": { "available": false, "reason": "not implemented" },
                    },
                    "targets": ["x86_64/raw/sysv-x86_64/none", "aarch64/raw/aapcs64/none", "aarch64/raw/apple-arm64/none"],
                    "engine_qualification": "not recorded yet",
                }),
                vec![],
            );
            0
        }
        ("expr", 3) if a.pos[1] == "check" => match expr::parse(&a.pos[2]) {
            Ok(e) => {
                emit("expr check", true, json!({ "normalized": e.to_string() }), vec![]);
                0
            }
            Err(m) => fail("expr check", anyhow::anyhow!("CONTRACT_TYPE_ERROR: {m}"), 2),
        },
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn main() {
    std::process::exit(real_main());
}
