// SPDX-License-Identifier: GPL-2.0-or-later
//! The mukoz-emu/1 protocol end to end: handshake, one run with a breakpoint event, reads after
//! the run, and an access outside the allowed ranges.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

struct P {
    tx: std::process::ChildStdin,
    rx: BufReader<std::process::ChildStdout>,
}

impl P {
    fn send(&mut self, v: Value) {
        writeln!(self.tx, "{v}").unwrap();
    }
    fn recv(&mut self) -> Value {
        let mut l = String::new();
        self.rx.read_line(&mut l).unwrap();
        serde_json::from_str(&l).unwrap()
    }
}

fn start() -> P {
    let mut c = Command::new(env!("CARGO_BIN_EXE_mukoz-emu")).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    P { tx: c.stdin.take().unwrap(), rx: BufReader::new(c.stdout.take().unwrap()) }
}

/// lea rax, [rdi+rsi]; mov [rdi], al; ret — at 0x100000, stack below 0x7fff0000.
fn run_spec(store_to: u64) -> Value {
    let sentinel = 0x0dea_d000u64;
    json!({
        "op": "run", "isa": "x86_64",
        "map": [{ "addr": 0x100000, "size": 0x1000, "perm": "rwx" }, { "addr": 0x7ffe0000u64, "size": 0x10000, "perm": "rw" }],
        "write": [{ "addr": 0x100000, "hex": "488d04378807c3" }, { "addr": 0x7ffeff00u64, "hex": hex(&sentinel.to_le_bytes()) }],
        "regs": [["rsp", 0x7ffeff00u64], ["rdi", store_to], ["rsi", 2]],
        "entry": 0x100000, "until": sentinel, "insn_limit": 100, "timeout_ms": 1000,
        "allowed": [[0x100000, 0x101000, 1, 0], [0x7ffe0000u64, 0x7ffeff08u64, 1, 1], [0x7ffe8000u64, 0x7ffe8010u64, 1, 1]],
        "code": [[0x100000, 0x100007]],
        "breaks": [0x100004]
    })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn hello_run_break_and_reads() {
    let mut p = start();
    p.send(json!({ "op": "hello" }));
    assert_eq!(p.recv()["protocol"], "mukoz-emu/1");
    p.send(run_spec(0x7ffe8000));
    let ev = p.recv();
    assert_eq!(ev["event"], "break");
    assert_eq!(ev["pc"], 0x100004);
    p.send(json!({ "op": "reg_read", "names": ["rax"] }));
    assert_eq!(p.recv()["regs"]["rax"], 0x7ffe8002u64);
    p.send(json!({ "op": "continue" }));
    let end = p.recv();
    assert_eq!(end["event"], "end", "{end}");
    assert!(end["stop"].is_null() && end["error"].is_null(), "{end}");
    assert_eq!(end["pc"], 0x0dea_d000u64);
    assert_eq!(end["count"], 3);
    p.send(json!({ "op": "mem_read", "addr": 0x7ffe8000u64, "len": 1 }));
    assert_eq!(p.recv()["hex"], "02");
    p.send(json!({ "op": "done" }));
    assert_eq!(p.recv()["ok"], true);
}

#[test]
fn an_access_outside_the_allowed_ranges_stops_the_run() {
    let mut p = start();
    p.send(run_spec(0x7ffeff10)); // mapped (stack page) but above the allowed stack range
    assert_eq!(p.recv()["event"], "break");
    p.send(json!({ "op": "continue" }));
    let end = p.recv();
    assert_eq!(end["stop"]["kind"], "access", "{end}");
    assert_eq!(end["stop"]["write"], true);
    assert_eq!(end["stop"]["addr"], 0x7ffeff10u64);
    p.send(json!({ "op": "done" }));
    assert_eq!(p.recv()["ok"], true);
}
