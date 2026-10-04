//! `mukoz` CLI (docs/07). Every command prints one JSON envelope.

mod emu;
mod image;
mod expr;
mod host;
mod judge;
mod native;
mod plan;
mod policy;
mod run;
mod spec;
mod store;

use judge::Admission;
use serde_json::json;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage:
  mukoz check <suite.toml> [--artifact <file>] [--module <name>=<file>]... [--fail-fast] [--gate] [--store <dir>] [--policy <policy.toml>]
  mukoz show <id> [--store <dir>]
  mukoz replay <counterexample-id> [--artifact <file>] [--store <dir>]
  mukoz inspect <file>
  mukoz regressions list <suite.toml> [--store <dir>]
  mukoz regressions prune <suite.toml> --case <id> [--store <dir>]
  mukoz platform probe | show [--store <dir>]
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
            "--artifact" | "--store" | "--case" | "--module" | "--policy" => {
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
    /// Every `--module name=path`.
    fn modules(&self) -> Result<Vec<(String, PathBuf)>, String> {
        self.opts
            .iter()
            .filter(|(n, _)| n == "--module")
            .map(|(_, v)| v.split_once('=').map(|(n, p)| (n.to_string(), PathBuf::from(p))).ok_or(format!("--module needs name=path, got `{v}`")))
            .collect()
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
            let modules = match a.modules() {
                Ok(m) => m,
                Err(e) => return fail("check", anyhow::anyhow!("USAGE: {e}"), 2),
            };
            let o = run::CheckOpts {
                policy: a.opt("--policy").map(Path::new),
                suite: Path::new(&a.pos[1]),
                artifact: a.opt("--artifact").map(Path::new),
                modules: &modules,
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
                    emit("inspect", true, inspect(p, &b), vec![]);
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
        ("platform", 2) if a.pos[1] == "probe" => match open_store(&a) {
            Ok(st) => {
                let p = host::probe();
                if let Err(e) = st.put_host("probe", &p) {
                    return fail("platform probe", e, 4);
                }
                emit("platform probe", true, p, vec![]);
                0
            }
            Err(e) => fail("platform probe", e, 4),
        },
        ("platform", 2) if a.pos[1] == "show" => match open_store(&a) {
            Ok(st) => {
                let quals: serde_json::Map<String, serde_json::Value> =
                    ["x86_64", "aarch64"].iter().filter_map(|i| st.get_host(&format!("qualification-{i}")).map(|q| (i.to_string(), q))).collect();
                emit(
                    "platform show",
                    true,
                    json!({
                        "host": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                        "mukoz": run::EVALUATOR_VERSION,
                        "probe": st.get_host("probe").unwrap_or(json!(null)),
                        "engine_qualification": quals,
                        "targets": {
                            "routine": ["x86_64/raw/sysv-x86_64/none", "aarch64/raw/aapcs64/none", "aarch64/raw/apple-arm64/none"],
                            "process": ["x86_64/raw/sysv-x86_64/linux", "x86_64/elf/sysv-x86_64/linux", "aarch64/raw/aapcs64/linux", "aarch64/elf/aapcs64/linux"],
                        },
                        "note": "`probe` is null until `mukoz platform probe` (or a check that needs it) ran with this store",
                    }),
                    vec![],
                );
                0
            }
            Err(e) => fail("platform show", e, 4),
        },
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

/// Static examination only: nothing is executed (docs/07 7.2).
fn inspect(p: &Path, b: &[u8]) -> serde_json::Value {
    let mut v = json!({
        "path": p.display().to_string(),
        "bytes": b.len(),
        "sha256": spec::sha256_hex(b),
        "head_hex": expr::hex(&b[..b.len().min(64)]),
    });
    if b.starts_with(b"\x7fELF") {
        let machine = b.get(18..20).map(|m| u16::from_le_bytes([m[0], m[1]]));
        let isa = match machine {
            Some(62) => Some(spec::Isa::X86_64),
            Some(183) => Some(spec::Isa::Aarch64),
            _ => None,
        };
        v["format"] = json!("elf");
        match isa {
            None => v["load"] = json!({ "ok": false, "error": format!("UNSUPPORTED_FEATURE: ELF e_machine {machine:?} (x86_64 = 62 and aarch64 = 183 are supported)") }),
            Some(isa) => {
                v["isa"] = json!(if isa == spec::Isa::X86_64 { "x86_64" } else { "aarch64" });
                v["load"] = match image::Image::elf(b, isa, u64::MAX) {
                    Ok(img) => json!({
                        "ok": true,
                        "target": format!("{}/elf/{}/linux", if isa == spec::Isa::X86_64 { "x86_64" } else { "aarch64" }, if isa == spec::Isa::X86_64 { "sysv-x86_64" } else { "aapcs64" }),
                        "entry": format!("0x{:x}", img.entry),
                        "segments": img.segments.iter().map(|s| json!({ "addr": format!("0x{:x}", s.addr), "file_bytes": s.data.len(), "mem_bytes": s.mem_size, "flags": s.label })).collect::<Vec<_>>(),
                        "use": "as a process: target <isa>/elf/<abi>/linux with [entry] kind = \"elf_entry\" (docs/13)",
                    }),
                    Err(e) => {
                        let m = format!("{e:#}");
                        json!({ "ok": false, "code": error_code(&m), "error": m })
                    }
                };
            }
        }
    } else if b.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]) || b.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) {
        v["format"] = json!("macho");
        v["load"] = json!({ "ok": false, "code": "UNSUPPORTED_FEATURE", "error": "UNSUPPORTED_FEATURE: Mach-O is not loaded yet" });
    } else if b.starts_with(b"MZ") {
        v["format"] = json!("pe");
        v["load"] = json!({ "ok": false, "code": "UNSUPPORTED_FEATURE", "error": "UNSUPPORTED_FEATURE: PE is not loaded" });
    } else {
        v["format"] = json!("raw");
        v["load"] = json!({ "ok": true, "use": "as raw code through a binding: a routine (<isa>/raw/<abi>/none) or a process image (<isa>/raw/<abi>/linux), entry by [entry] offset" });
    }
    v
}

fn main() {
    std::process::exit(real_main());
}
