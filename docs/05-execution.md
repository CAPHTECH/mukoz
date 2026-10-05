# 05 Artifact inspection and execution

## 5.1 Inspector

Inspection always runs against an immutable snapshot. A per-format module builds a common `LoadPlan`.

```text
size limit → format detection → header → ISA/slice → load info (segments, etc.)
          → range / integer overflow checks → entry resolution → dependency / relocation detection
          → consistency with target platform → LoadPlan
```

`LoadPlan` does not depend on the format. It holds: the regions to place (virtual address, size, byte range from the file, zero-fill range, permissions), the virtual address of the entry, the list of unresolved dependencies, and the unsupported features detected.

### Per-format inspection

| Format | Main checks | Entry | Initial support scope |
|---|---|---|---|
| raw | None (the Binding supplies the virtual base and the entry offset) | Binding offset | Supported |
| ELF | ident, class, endian, machine; range and overlap of program headers; `p_filesz ≤ p_memsz`; presence of `PT_INTERP` and `PT_DYNAMIC` | `e_entry` (virtual address) | Statically linked `ET_EXEC` executables. If `PT_INTERP` is present: `UNRESOLVED_DEPENDENCY` |
| Mach-O | magic, CPU type; `cmdsize` of load commands and total length; file/vm ranges of segments; `__PAGEZERO`; dylib dependencies | `LC_MAIN.entryoff` (a file offset from `__TEXT`, not a virtual address) or `LC_UNIXTHREAD` | Simple, nearly static executables. If there are dyld dependencies: `UNRESOLVED_DEPENDENCY` |
| PE | DOS/PE header, section ranges, import table | `AddressOfEntryPoint` (RVA) | Structural inspection only at first. Execution comes in a later stage |

- The `object` crate is a candidate for reading formats. A library being able to read a file does not mean the inspection is complete. Mukoz performs the consistency checks itself.
- Do not allocate real memory for huge virtual regions such as `__PAGEZERO`. Check the guest memory limit before placement.
- For signature regions, check only the range. This is separate from cryptographic signature verification and from the OS's judgement of distributed software.

### Disassemble from the entry

Do not decide the code range by disassembling linearly from the start of `.text` (there are cases where instructions and strings are mixed in the same section). x86_64 has variable-length instructions, so start in particular from the entry, the declared code regions, and the PCs actually reached. Capstone is used for display and as an aid; it is not the basis of execution semantics.

## 5.2 Execution lifecycle

```text
Created → Prepared → Running → Completed
                        ├──→ BudgetExhausted
                        ├──→ Cancelled
                        ├──→ WorkerFailed
                        └──→ UnsupportedDuringRun
```

`Completed` means that processing finished. It is not a pass/fail result. Abnormal termination of the subject and abnormal termination of the checker are recorded separately.

Guest state is initialized for each case. Snapshot restoration for speed is introduced only after tests show that registers, memory, flags, the effect model, and the engine's translation cache do not leak from the previous case.

## 5.3 Routine invocation and completion

### Procedure (common to all ISAs)

1. Place the regions of the LoadPlan and the Binding, and set permissions. Put guards between regions.
2. Initialize registers and regions from the input.
3. Prepare the return sentinel and SP according to the rules of the ISA and ABI (table below).
4. Registers that the Binding does not specify get seed-fixed values, within what does not violate the ABI. **Do not set them all to 0** (this would miss errors that depend on zero initialization).
5. Take an entry snapshot and fix the baseline values of pointer arguments.
6. Run while monitoring instructions, memory accesses, traps, and the budget.
7. Evaluate the contract and the machine claims from the snapshot at return and the effect record.

The concrete initial values of registers and memory are saved as part of the case. Reproduction does not rely on the seed alone.

### Preparing the return target and completion verdict

| ISA | Return target setup | Completion condition |
|---|---|---|
| aarch64 | `x30` (LR) = sentinel; SP is 16-byte aligned | PC = sentinel, and SP = SP at entry |
| x86_64 | Push the sentinel on the stack. At entry `rsp ≡ 8 (mod 16)` (the state right after a call) | PC = sentinel, and RSP = RSP at entry + 8 |

- Do not simply "finish at the first `ret`". That would end at the `ret` of another routine called along the way.
- The sentinel is a dedicated non-executable address, and reaching it is caught by an engine hook.
- If control moves anywhere other than the return target (a jump outside the code region, etc.), stop tracking and do not report success. If the contract forbids it, it is a counterexample. If it is an unsupported control transfer, it is `UNSUPPORTED_DURING_RUN`.

## 5.4 ABI table

ABIs are held as versioned data. The versions of the ISA, the ABI, and the OS are recorded separately.

| ABI | Arguments (integer) | Return value | Preserved (callee-saved) | Stack | Other |
|---|---|---|---|---|---|
| `sysv-x86_64` | rdi, rsi, rdx, rcx, r8, r9 | rax(, rdx) | rbx, rbp, r12–r15, rsp | 16-byte aligned at call. The 128 bytes below RSP (red zone) are writable | DF=0 at entry and exit |
| `win64` | rcx, rdx, r8, r9 | rax | rbx, rbp, rdi, rsi, r12–r15, rsp, xmm6–xmm15 | 16-byte aligned. The caller reserves 32 bytes of shadow space. No red zone | — |
| `aapcs64` | x0–x7 | x0(, x1) | x19–x28, x29, SP, lower 64 bits of v8–v15 | SP is always 16-byte aligned | x18 is the platform register (its use is decided by the OS) |
| `apple-arm64` | Same as aapcs64 | Same | Same | Same | x18 is reserved (must not be used). x29 points to a valid frame record |
| `win-arm64` | Same as aapcs64 | Same | Same | Same | x18 is reserved (TEB) |

**Note:** This table is `[U]`: it was not checked against primary sources when this document was written. At implementation time, confirm it against the AAPCS64, Apple, Microsoft, and System V psABI documents, and record the document versions in the ABI data.

- Initial inputs and outputs are limited to integers of 64 bits or less and pointers. Variadic arguments, struct-by-value, and floating-point arguments are `UNSUPPORTED_FEATURE`.
- In configurations that do not check preservation of SIMD/FP registers, that claim is NOT_EVALUATED, and the result is not reported as "the whole ABI passes".
- **Handling of the red zone:** In `sysv-x86_64`, a write to the 128 bytes below the entry RSP is a legitimate write within the stack region. In `win64`, it is not. The permitted range of the stack region is computed from the ABI.
- "Restored on return" and "must not be used" are separate claims (`callee_saved` and `reserved`).

## 5.5 Executor implementation

### emulated

- The first candidate was Unicorn; the default engine is now icicle-emu (ADR-25). It handles x86_64 and aarch64 with the same engine. The adoption conditions are passing the engine qualification on each host and a license check (chapter 10).
- Hooks: instructions (whether the PC is inside the code region, sentinel reached), memory accesses (whether the half-open interval `[addr, addr+width)`, including the width, fits entirely within a permitted region), interrupt and syscall instructions (to the effect model), invalid instructions and unmapped accesses.
- Do not automatically allocate a page on access to an unmapped address. Do not fill undefined regions with 0.
- Implementation (0.1.0): the engine runs in a separate program (chapter 03 §3.3, `emu/PROTOCOL.md`). The default is `mukoz-emu-icicle`: icicle-emu, a SLEIGH / p-code interpreter that shares no code with Unicorn or QEMU (ADR-25). `mukoz-emu` runs Unicorn and is built only on request (`-p mukoz-emu`); `MUKOZ_EMU` selects it. `emu-icicle/README.md` lists the differences between the two. The engine identity recorded in the subject context and in the engine qualification is what the program reports at the handshake. Without an emulator program, emulated results are HOLD (`ENGINE_NOT_QUALIFIED`).
- Observation (linux-x86_64, one run each): both programs pass the engine qualification on both ISAs, `cargo test --release` and `tools/tier1.py`. For `popcnt`, the qualification accepts either an invalid-instruction stop or the correct value: Unicorn does not implement it, icicle-emu does. A wrong value fails either way.
- Per-instruction and per-access hooks are slow `[R]` (emulator hooks generally prevent fast execution of translated blocks; performance is unmeasured). Configure the required monitoring and the saving of detailed traces separately.

### native-routine (host ISA = target ISA)

The target code is run on the real CPU in a child process. It is not loaded into the Mukoz process itself.

1. In the child process, allocate memory, copy the code in, then make it executable and non-writable (W^X).
2. A small trampoline sets the registers and branches to the entry. After the return, it saves all registers.
3. Invalid accesses and invalid instructions are caught by signals (exceptions on Windows) and recorded as `crashed`.
4. The child process is terminated at the time limit.

| Capability | Possible? |
|---|---|
| Return value, preserved registers, SP | Yes |
| Crash detection | Yes |
| Out-of-region access | Only what guard pages can detect (page granularity). Not at byte granularity |
| Attempts at forbidden effects (syscalls) | Not initially (on Linux it is expected to be addable later with seccomp `[R]`) |

It is used as the counterpart for differential tests against `emulated` (chapter 06 §6.8).

### native-process / translated-process

- Do not use a shell. Launch a fixed executable with structured argv. The environment variables are empty by default, and only what the Binding specifies is passed.
- Read stdout and stderr at the same time, and supervise the output limit, pipe stalls, and waits caused by descendant processes keeping an fd open.
- What is guaranteed to be observed is limited to: the byte sequences of stdout/stderr of the directly launched process, the exit status or signal, elapsed time, and the classification of launch failures.
- Even if a before/after comparison of files is the same, temporary writes and external transmission cannot be ruled out.

| OS | Launch | Stopping descendants | Pinning the executed file |
|---|---|---|---|
| Linux | `posix_spawn` / `fork+exec`, new process group | Kill a child cgroup of cgroup v2 (if delegated). If not, process group only | Copy the snapshot into a memfd and launch with `execveat`/`fexecve`. The path is not re-resolved `[R]` |
| macOS | `posix_spawn`, process group | Process group only (descendants that have detached may not be stoppable) | Copy into a dedicated directory and restrict permissions. Record that room for path re-resolution remains |
| Windows | `CreateProcess` (arguments are assembled in structured form) | Job Object (kill-on-close) | Copy into a dedicated directory |

- If execution is stopped by an OS execution policy (macOS Gatekeeper, Windows SmartScreen / Mark-of-the-Web, a Linux `noexec` mount, etc.), the result is `EXECUTION_BLOCKED_PLATFORM_POLICY`. It is not treated as a functional counterexample.
- Mukoz does not remove quarantine attributes, add signatures, or change OS security settings. The owner prepares the file outside Mukoz and registers the changed file.
- `translated-process` records the name and version of the translation layer (qemu-user, Rosetta 2, etc.) in `execution_platform`. The current host has no qemu-user, so it is reported as having no capability at first.

## 5.6 Effect model

The **meaning** of an effect is separated from the **syscall adapter** for each OS×ISA.

```text
trap (syscall instruction)
   │
   ▼
syscall adapter (OS×ISA): translate number, argument registers, return value, error representation
   │
   ▼
effect meaning (OS-independent): write(fd, bytes) / exit(code) / …
   │
   ├─ permitted effect → the environment model decides the response and records an effect event
   └─ anything else    → stop as an attempt at a forbidden effect (or stop as unsupported)
```

Guest syscalls are not forwarded to the host OS. Showing output on the host's stdout and modeling the guest's writes to its stdout are different things.

### Adapter table (initial)

| adapter | trap instruction | Number | Arguments | Return value / error | Example numbers | Verification status |
|---|---|---|---|---|---|---|
| `linux-x86_64` | `syscall` | rax | rdi, rsi, rdx, r10, r8, r9 | rax; errors as `-errno` | write=1, exit=60, exit_group=231 | write=1 and exit=60 observed once by running a hand-written hello. Others `[U]` |
| `linux-aarch64` | `svc #0` | x8 | x0–x5 | x0; errors as `-errno` | write=64, exit=93, exit_group=94 | `[U]` |
| `darwin-aarch64` | `svc #0x80` | x16 | x0–x5 | x0; on error, set the carry flag and put errno in x0 | write=4, exit=1 | Numbers quoted by design version 0.4 from XNU's `syscalls.master`. Others `[U]` |
| `darwin-x86_64` | `syscall` | rax (with class prefix `0x2000000`) | rdi, rsi, rdx, r10, r8, r9 | rax; carry flag on error | write=4, exit=1 | `[U]` |
| Windows | — | — | — | — | — | Syscall numbers are not published as a stable ABI `[R]`. A stub model per API (kernel32 / ntdll) is needed, and is out of the initial scope |

At implementation time, fix the numbers against each OS's primary sources (the Linux kernel syscall table, XNU's `syscalls.master`), and record the document versions in the adapter.

### Environment models

| Model | Content |
|---|---|
| `linux-stdio/1` | Only write, exit, and exit_group. The stack at process start (argc, argv, envp, minimal auxv) is built with the construction fixed as the model's version |
| `darwin-stdio/1` | Only write and exit. Calls the `LC_MAIN` entry as `main(argc, argv, envp, apple)` and treats a return to the sentinel as `exit(x0)`. This is an **assumption** about the behavior of dyld and libSystem, and is stated explicitly in the evidence |
| `full-success/1` | Every write succeeds for the requested length |
| `scripted-write-results/1` (later stage) | Injects short writes and failures by script |

- The effect model records the fd, the buffer, the requested length, the range read, the number of bytes accepted, the return value, and the event order. The requested length is not treated as the length written.
- Do not substitute a success response for an unimplemented failure response.
- An executable statically linked against glibc or similar calls many syscalls at startup (brk, arch_prctl, etc.) `[R]`. In the initial model this results in `UNSUPPORTED_DURING_RUN`. The initial process fixtures are hand-written assembly that does not use libc.
- The result of `darwin-stdio/1` does not mean that dyld initialization, initializer functions, or library loading were inspected.

## 5.7 Handling of errors

| Event | Handling |
|---|---|
| Access to an unmapped address | Do not allocate a page automatically. If the contract or Binding forbids it, a counterexample; if it is outside the model, inconclusive |
| Unsupported instruction | `UNSUPPORTED_DURING_RUN`. Do not replace it with a NOP |
| Unsupported syscall | If it is an effect forbidden by the contract, a counterexample; otherwise `UNSUPPORTED_DURING_RUN` |
| Self-modifying code | Not supported. Treated as a W^X violation: a counterexample if the contract forbids it, otherwise inconclusive |

W^X, restrictions on branch targets, and the like are shown as check conditions of that Executor and environment model, not as "rules for the correctness of the whole OS".
