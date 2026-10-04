//! `show <id> [--page N] [--disasm]` (docs/07 7.2, 7.6): items are returned in pages of at most
//! 32 elements per list, so an agent can go from a run summary to a finding, its counterexample
//! and the instruction trace without reading everything at once.

use crate::store::Store;
use anyhow::Result;
use serde_json::{Value, json};

pub const PAGE_ITEMS: usize = 32;

/// Cut every list longer than PAGE_ITEMS to page `page` (1-based) and record what was cut.
fn paginate(v: &mut Value, path: &str, page: usize, cut: &mut Vec<Value>) {
    match v {
        Value::Array(a) => {
            if a.len() > PAGE_ITEMS {
                let total = a.len();
                let pages = total.div_ceil(PAGE_ITEMS);
                let lo = (page - 1) * PAGE_ITEMS;
                let kept: Vec<Value> = a.iter().skip(lo).take(PAGE_ITEMS).cloned().collect();
                cut.push(json!({ "path": path, "total": total, "page": page, "pages": pages }));
                *a = kept;
            }
            for (i, x) in a.iter_mut().enumerate() {
                paginate(x, &format!("{path}/{i}"), page, cut);
            }
        }
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                paginate(x, &format!("{path}/{k}"), page, cut);
            }
        }
        _ => {}
    }
}

#[cfg(feature = "disasm")]
fn disasm_one(target: &str, bytes: &[u8], addr: u64) -> String {
    use capstone::prelude::*;
    let cs = if target.starts_with("aarch64") {
        Capstone::new().arm64().mode(arch::arm64::ArchMode::Arm).build()
    } else {
        Capstone::new().x86().mode(arch::x86::ArchMode::Mode64).syntax(arch::x86::ArchSyntax::Intel).build()
    };
    let Ok(cs) = cs else { return "(capstone unavailable)".into() };
    match cs.disasm_count(bytes, addr, 1) {
        Ok(insns) => insns.iter().next().map(|i| format!("{} {}", i.mnemonic().unwrap_or("?"), i.op_str().unwrap_or(""))).unwrap_or_else(|| "(undecodable)".into()),
        Err(_) => "(undecodable)".into(),
    }
}

#[cfg(not(feature = "disasm"))]
fn disasm_one(_: &str, _: &[u8], _: u64) -> String {
    "(built without the disasm feature)".into()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}

/// Add `asm` to every `{offset, bytes}` instruction record under `v`.
fn annotate(v: &mut Value, target: &str) {
    match v {
        Value::Object(m) => {
            if let (Some(Value::String(off)), Some(Value::String(b))) = (m.get("offset"), m.get("bytes")) {
                let addr = off.rsplit("0x").next().and_then(|h| u64::from_str_radix(h, 16).ok()).unwrap_or(0);
                let text = disasm_one(target, &unhex(b), addr);
                m.insert("asm".into(), json!(text));
                return;
            }
            for (_, x) in m.iter_mut() {
                annotate(x, target);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| annotate(x, target)),
        _ => {}
    }
}

pub fn show(store: &Store, id: &str, page: Option<usize>, disasm: bool) -> Result<Value> {
    let mut item = store.get_item(id)?;
    let page = page.unwrap_or(1).max(1);
    if disasm {
        let target = item["target"].as_str().or_else(|| item["data"]["assessment"]["scope"]["target"].as_str()).unwrap_or("x86_64").to_string();
        annotate(&mut item, &target);
        item["disassembly"] = json!({ "engine": if cfg!(feature = "disasm") { "capstone 0.14" } else { "none" }, "note": "diagnostic only; verdicts never depend on it" });
    }
    let mut cut = Vec::new();
    paginate(&mut item, "", page, &mut cut);
    if !cut.is_empty() {
        item["_pages"] = json!({ "page": page, "items_per_page": PAGE_ITEMS, "lists": cut, "next": format!("mukoz show {id} --page {}", page + 1) });
    }
    Ok(item)
}
