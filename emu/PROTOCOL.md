# mukoz-emu/1

`mukoz-emu` is a machine emulator program. A client starts it and talks to it over stdin /
stdout, one JSON object per line in each direction. The emulator knows nothing about contracts,
bindings or verdicts: it maps memory, sets registers, runs code, stops on accesses outside the
ranges it is given, and asks the client what to do at system calls, interrupts and breakpoints.
`mukoz` is one client; any program can be.

Numbers are unsigned 64-bit JSON integers. Byte strings are lowercase hex (`"hex": "c3"`).
Registers are named: x86-64 `rax rbx rcx rdx rsi rdi rbp rsp r8`–`r15 rip rflags`; AArch64
`x0`–`x30 sp pc nzcv`.

## Session

| Client sends | Emulator answers |
|---|---|
| `{"op":"hello"}` | `{"protocol":"mukoz-emu/1","engine":"<engine and version>","version":"<mukoz-emu version>"}` |
| `{"op":"run", …}` | events (below), ending with `{"event":"end", …}` or `{"event":"setup_error","error":…}` |
| `{"op":"quit"}` | exits. End of input also ends the program. |

`mukoz-emu --version` prints the same identity and exits.

## run

```json
{"op":"run","isa":"x86_64",
 "map":[{"addr":1048576,"size":4096,"perm":"rwx"}],
 "write":[{"addr":1048576,"hex":"488d0437c3"}],
 "regs":[["rsp",2147418048],["rdi",1],["rsi",2]],
 "entry":1048576,"until":233492480,"insn_limit":10000,"timeout_ms":1000,
 "allowed":[[lo,hi,read,write]],
 "code":[[lo,hi]],
 "frozen":{"x18":0},
 "breaks":[addr]}
```

| Field | Meaning |
|---|---|
| `isa` | `x86_64` or `aarch64` |
| `map` | Pages to map; `perm` is `rwx` or `rw`. Nothing else is mapped. |
| `write` | Initial memory contents, in order. |
| `regs` | Initial registers, in the order given. |
| `entry`, `until` | Start address; the run ends when execution reaches `until`. |
| `insn_limit`, `timeout_ms` | Instruction budget and wall-clock limit. |
| `allowed` | Data accesses must fall entirely inside one range `[lo, hi)` that allows that kind of access (`read` / `write` are 0 or 1). Any other access stops the run. |
| `code` | Every executed instruction must lie inside one of these ranges. |
| `frozen` | Registers that must keep these values; a change stops the run. |
| `breaks` | Addresses at which the emulator reports a `break` event before executing the instruction. |

A fresh engine instance is created for every `run`; nothing carries over between runs.

## Events during a run

Each event waits for the client. The client may send any number of machine operations, then
`{"op":"continue"}` (resume) or `{"op":"stop"}` (end the run now; the end event then has stop
kind `client`). Neither of these two gets a reply.

| Event | When |
|---|---|
| `{"event":"break","pc":…,"sp":…}` | before executing an instruction at a breakpoint address |
| `{"event":"syscall","insn":"syscall"\|"sysenter","pc":…}` | x86-64 system call instruction |
| `{"event":"interrupt","intno":…,"pc":…}` | interrupt or exception (A64 `svc` = 2, undefined instruction = 1, `brk`; x86 `int`) |

`pc` in `syscall` and `interrupt` is the address of the last instruction started.

## Machine operations

Allowed during an event and after the end event.

| Request | Reply |
|---|---|
| `{"op":"reg_read","names":["rax",…]}` | `{"regs":{"rax":…}}` or `{"error":…}` |
| `{"op":"reg_write","regs":{"rax":…}}` | `{"ok":true}` or `{"error":…}` |
| `{"op":"mem_read","addr":…,"len":…}` | `{"hex":…}` or `{"error":…}` (at most 64 MiB) |
| `{"op":"mem_write","addr":…,"hex":…}` | `{"ok":true}` or `{"error":…}` |
| `{"op":"break_add","addr":…}` / `{"op":"break_remove","addr":…}` | `{"ok":true}` |

Machine operations ignore `allowed`: they are the client's own accesses.

## End of a run

```json
{"event":"end","stop":null,"error":null,"pc":…,"sp":…,"count":3,"elapsed_ms":0,"ring":[[addr,size],…]}
```

| Field | Meaning |
|---|---|
| `stop` | Why the emulator stopped the run, or null. The first reason wins. Kinds: `undecodable {pc}`, `left_code {from, target}` (an instruction or fetch outside `code`), `frozen {name, pc}`, `access {write, addr, size, pc}` (outside `allowed`), `fault {write, addr, size, pc}` (unmapped or protected), `client`. |
| `error` | The engine's error if it ended with one (for example `INSN_INVALID`), else null. |
| `pc`, `sp` | Program counter and stack pointer at the end. |
| `count` | Instructions executed. |
| `elapsed_ms` | Wall-clock time of the run. |
| `ring` | The last 64 instructions executed (address, size), oldest first. |

When `stop` and `error` are both null, the run reached `until`, or the budget or the time limit
ended it (compare `count` with `insn_limit` and `elapsed_ms` with `timeout_ms`).

After the end event the client may read registers and memory, and ends the run with
`{"op":"done"}`, answered by `{"ok":true}`. The emulator is then ready for the next `run`.
