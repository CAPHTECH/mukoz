// SPDX-License-Identifier: MIT OR Apache-2.0
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
    let mut c = Command::new(env!("CARGO_BIN_EXE_mukoz-emu-icicle")).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
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

/// Run `code` (x86-64) at 0x100000 with a small stack; returns the end event after answering
/// every event with `answer`.
fn run_x86(p: &mut P, code: &str, regs: Value, insn_limit: u64, mut answer: impl FnMut(&mut P, &Value) -> &'static str) -> Value {
    let sentinel = 0x0dea_d000u64;
    let mut r = vec![json!(["rsp", 0x7ffeff00u64])];
    r.extend(regs.as_array().cloned().unwrap_or_default());
    p.send(json!({
        "op": "run", "isa": "x86_64",
        "map": [{ "addr": 0x100000, "size": 0x1000, "perm": "rwx" }, { "addr": 0x7ffe0000u64, "size": 0x10000, "perm": "rw" }],
        "write": [{ "addr": 0x100000, "hex": code }, { "addr": 0x7ffeff00u64, "hex": hex(&sentinel.to_le_bytes()) }],
        "regs": r,
        "entry": 0x100000, "until": sentinel, "insn_limit": insn_limit, "timeout_ms": 1000,
        "allowed": [[0x100000, 0x101000, 1, 0], [0x7ffe0000u64, 0x7fff0000u64, 1, 1]],
        "code": [[0x100000, 0x101000]],
        "breaks": [0x100000]
    }));
    loop {
        let ev = p.recv();
        if ev["event"] == "end" || ev["event"] == "setup_error" {
            return ev;
        }
        let op = answer(p, &ev);
        p.send(json!({ "op": op }));
    }
}

#[test]
fn a_system_call_resumes_after_the_instruction() {
    let mut p = start();
    // mov eax, 60; syscall; mov eax, 7; ret
    let mut seen = Vec::new();
    let end = run_x86(&mut p, "b83c0000000f05b807000000c3", json!([]), 100, |p, ev| {
        seen.push(ev["event"].as_str().unwrap().to_string());
        if ev["event"] == "syscall" {
            assert_eq!(ev["insn"], "syscall");
            assert_eq!(ev["pc"], 0x100005);
            p.send(json!({ "op": "reg_read", "names": ["rax"] }));
            assert_eq!(p.recv()["regs"]["rax"], 60);
        }
        "continue"
    });
    assert_eq!(seen, ["break", "syscall"]);
    assert!(end["stop"].is_null() && end["error"].is_null(), "{end}");
    assert_eq!(end["pc"], 0x0dea_d000u64);
    assert_eq!(end["count"], 4);
    p.send(json!({ "op": "reg_read", "names": ["rax"] }));
    assert_eq!(p.recv()["regs"]["rax"], 7);
    p.send(json!({ "op": "done" }));
    p.recv();
}

#[test]
fn the_budget_ends_a_loop_and_rflags_round_trips() {
    let mut p = start();
    // std; jmp $ (DF set, then an endless loop)
    let end = run_x86(&mut p, "fdebfe", json!([["rflags", 0x202]]), 50, |_, _| "continue");
    assert!(end["stop"].is_null() && end["error"].is_null(), "{end}");
    assert_eq!(end["count"], 50);
    p.send(json!({ "op": "reg_read", "names": ["rflags"] }));
    assert_eq!(p.recv()["regs"]["rflags"], 0x602);
    p.send(json!({ "op": "done" }));
    p.recv();
}

#[test]
fn a_client_jump_during_a_break_is_followed() {
    let mut p = start();
    // mov eax, 1; ret; (0x100006:) mov eax, 2; ret — the client jumps to 0x100006 at the break.
    let end = run_x86(&mut p, "b801000000c3b802000000c3", json!([]), 100, |p, _| {
        p.send(json!({ "op": "reg_write", "regs": { "rip": 0x100006 } }));
        p.recv();
        "continue"
    });
    assert!(end["stop"].is_null() && end["error"].is_null(), "{end}");
    p.send(json!({ "op": "reg_read", "names": ["rax"] }));
    assert_eq!(p.recv()["regs"]["rax"], 2);
    p.send(json!({ "op": "done" }));
    p.recv();
}

#[test]
fn an_undecodable_instruction_and_leaving_the_code_stop_the_run() {
    let mut p = start();
    let end = run_x86(&mut p, "0f0b", json!([]), 100, |_, _| "continue"); // ud2
    assert_eq!(end["stop"]["kind"], "undecodable", "{end}");
    p.send(json!({ "op": "done" }));
    p.recv();
    // mov rax, 0x7ffe0000; jmp rax (into the stack page, which is not executable)
    let end = run_x86(&mut p, "48c7c00000fe7fffe0", json!([]), 100, |_, _| "continue");
    assert_eq!(end["stop"]["kind"], "left_code", "{end}");
    assert_eq!(end["stop"]["target"], 0x7ffe0000u64);
    p.send(json!({ "op": "done" }));
    p.recv();
}
