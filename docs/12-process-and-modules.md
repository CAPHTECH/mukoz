# 12. Process boundary and module splitting (implementation)

This chapter defines the mechanism for checking artifacts larger than a single routine (programs that use system calls, implementations split into several modules). It describes the **implemented scope**. Where it disagrees with the design in chapters 04 and 05, this chapter (and the implementation's error messages) is correct.

## 12.1 Aims

- Write a command-line application (arguments, standard input/output, files, exit code) as a contract and make it checkable with Mukoz.
- **Split** a large implementation **into modules**, check each module, and when they are combined, report the **blame** for a defect (caller or callee) separately.

## 12.2 Process boundary

### Target

`<isa>/<format>/<abi>/linux`. `format` is `raw` (the start, or `[entry] offset`, is the entry) or `elf` (static ET_EXEC; `kind = "elf_entry"`). There is also `aarch64/macho/apple-arm64/darwin` (§12.6).

| Item | Content |
|---|---|
| Supported | x86-64 (sysv-x86_64) and AArch64 (aapcs64). Both use Linux conventions |
| ELF | 64-bit, little-endian, ET_EXEC only. PT_INTERP / PT_DYNAMIC give `UNRESOLVED_DEPENDENCY`; ET_DYN (PIE) and PT_TLS give `UNSUPPORTED_FEATURE`; loading stops at load time. Segments are permitted exactly as p_flags R/W/X say |
| raw | The code is placed at `0x100000`, read-only. As a writable region, `[process] data_bytes` (default 64 KiB, 0 for none) is placed from `0x10000000`, filled with 0 |
| Initial state | Same as Linux: sp → argc, argv[], NULL, envp NULL, auxv AT_NULL. Strings are at the top of the stack. sp is 16-byte aligned. General-purpose registers are 0 |
| Stack | Default 64 KiB (`[stack] bytes`) |

### System calls (effect model)

Numbers follow the Linux x86-64 table and the asm-generic table (AArch64). The call is `syscall` on x86-64 (rax; rdi/rsi/rdx/r10) and `svc` on AArch64 (x8; x0–x3).

| Effect | x86-64 / AArch64 | Model |
|---|---|---|
| read | 0 / 63 | fd 0 is stdin; any other is an opened file |
| write | 1 / 64 | fd 1 and 2 are stdout and stderr; any other is a file |
| open / openat | 2, 257 / 56 | Only paths declared in `[files]`. Interprets O_CREAT, O_TRUNC, O_APPEND, O_EXCL. An undeclared path gives ENOENT (EACCES with O_CREAT). dirfd is ignored |
| close | 3 / 57 | |
| lseek | 8 / 62 | SEEK_SET / CUR / END. stdin etc. give ESPIPE |
| exit / exit_group | 60, 231 / 93, 94 | The exit code is the low 8 bits |

- In the contract's `[effects] allow`, write whichever of `read`, `write`, `open`, `close`, `lseek` are needed. exit is always allowed. A call to an effect that is not allowed is a violation of `effects.no_forbidden`.
- **A system call not in the table above gives `UNSUPPORTED_DURING_RUN` (HOLD).** It is never silently treated as a success (chapter 01 P5).
- If a buffer or path passed to a system call lies outside permitted memory, it is a memory violation at that point (Linux would return EFAULT, but the check stops it as a defect).
- The upper bound on a file's size is the `max_len` of the observed contract state variable. A write beyond it gives ENOSPC. The upper bound on stdout and stderr is the `max_len` of the result variable; exceeding it is a violation of `effects.output_within_limit`.

### Contract and Binding

```toml
# contract.toml
boundary = "process"
modifies = ["db", "db_exists"]
[inputs]   op = "bv8"; nargs = "bv64"; text = { type = "bytes", max_len = 24 }
[state]    db = { type = "bytes", max_len = 600 }; db_exists = "bool"
[results]  code = "bv8"; out = { type = "bytes", max_len = 1024 }; err = { type = "bytes", max_len = 64 }
[effects]  allow = ["read", "write", "open", "close"]
[termination] kind = "must_exit"
```

```toml
# binding.toml
target = "x86_64/elf/sysv-x86_64/linux"
[entry] kind = "elf_entry"
[process]
argv0 = "todo"
argv = ['ite(input.op == bv8(0), b"add", b"list")', "input.text"]   # bytes expressions. Must not contain NUL
argc = "input.nargs"           # number of argv entries used (excluding argv0). BINDING_MISMATCH if it exceeds the number of argv entries
stdin = "input.data"           # empty if omitted
[files.db]
path = "todo.db"
init = "before.db"             # initial content
exists = "before.db_exists"    # true if omitted
observe_as = "after.db"        # content at exit (empty if it does not exist)
exists_as = "after.db_exists"  # whether it exists at exit
[results] code = "exit_status"; out = "stdout"; err = "stderr"
[completion] kind = "exit"
```

- A process result can have the bytes type (stdout, stderr).
- The machine properties are `machine.exited` (ended with exit; it is a violation if control leaves some other way, such as a ret from the entry), `machine.memory.access`, `effects.no_forbidden`, and `effects.output_within_limit`.
- A counterexample's `observed` shows stdout and stderr (as text and hex), the exit code, each file, and the last 16 system calls (arguments and return values).

## 12.3 Module splitting

### The link file

```toml
schema = "mukoz.link/1"

[[modules]]
name = "main"
path = "main.bin"
exports = { start = 0, helper = 0x40 }   # symbol = offset within the module

[[modules]]
name = "store"
path = "store.bin"
exports = { load = 0, save = 0x80 }

[imports]            # slot number = "module.symbol"
0 = "store.load"
1 = "store.save"

[monitors]           # callee symbol = that symbol's Suite (the routine's contract and Binding)
"store.load" = "store_load/suite.toml"
```

- **Layout:** the i-th module is placed at `0x100000 + i × 0x100000`, read-only and executable (at most 16, each up to 1 MiB).
- **Import table:** at `0xf0000`, 8 bytes per slot, holding absolute addresses in slot order (read-only, 512 slots). There is no relocation. Calls go through the table:
  - x86-64: `call qword ptr [0xf0000 + 8*k]` (`ff 14 25 <disp32>`)
  - AArch64: `movz x16, #0xf, lsl #16` → `ldr x16, [x16, #8*k]` → `blr x16`
- In the Binding, write `link = "link.toml"` and `[entry] kind = "symbol"`, `symbol = "main.start"`. It works for both routines and processes (for a process, raw only).
- `--artifact <file>` replaces the entry module's file, and `--module <name>=<file>` replaces any module's file (repeatable).
- A counterexample's location is shown as "module name + offset", such as `store+0x1c`.

### How to proceed

1. Build from the leaf modules and check each **on its own** with a Suite (a routine contract) (pass that module's file with `--artifact`).
2. Check an upper module by linking the **real** lower modules. If you write the lower Suites in `[monitors]`, the lower contracts are also checked on every call.
3. To check only the upper module with the lower one replaced, supply another implementation known to be correct with `--module`. Automatically generating stubs from a contract is not implemented (chapter 10).

## 12.4 Boundary monitoring and blame

Each time control reaches the entry of a symbol written in `[monitors]`, the contract's values are recovered by using that symbol's Binding in reverse, and the contract is evaluated.

| Property | Meaning | Blame |
|---|---|---|
| `link.<symbol>.requires` | At call time, the callee's requires is false | **Caller** |
| `link.<symbol>.ensures` | At return, the callee's ensures or frame is false (only calls whose requires held at call time are evaluated. The result of a call that broke requires is not blamed on the callee) | **Callee** |
| `link.<symbol>.abi` | At return, a callee-saved register had changed | **Callee** |

- Value recovery: values can be recovered only when an argument has the form `input.x` (bv, bool), `len(input.b)`, or `addr(region)`. The size of a region comes from the argument of `len(...)`; if that is not possible, write `monitor_size = "<argument expression>"` on the Binding's region (for example, if the size of dst equals len(src) by requires, `monitor_size = "len(input.src)"`). Inputs that the contract's requires and ensures do not reference (for example, inputs used only by the Suite's generator to build structured values) need not be recovered. A Binding whose referenced variables cannot be recovered gives `MONITOR_UNSUPPORTED` at load time.
- Detecting the return: when the return address recorded at call time (`[rsp]` on x86-64, x30 on AArch64) is reached with the post-return sp. Recursion and nesting are also tracked.
- In a case where the callee was never called, that monitor's property is not evaluated. If it is never called in any case, the result is NOT_EVALUATED (HOLD).
- The violation `detail` shows `blame`, the call count, `why_false`, and, for a requires violation, the return address (`return_to`, just after the call site).

**What it cannot do (not implemented):**
- Detecting the callee overwriting caller memory (such as the caller's stack) that lies outside its own contract's regions. Only the whole-process memory monitor (whether access is to a permitted region) works.
- Stub generation from a contract (checking the caller without implementing the callee).
- Module splitting of ELF (dynamic linking, relocation).

## 12.5 What was confirmed

| What | Result |
|---|---|
| hello (x86-64 raw, AArch64 raw, AArch64 ELF) | All three ACCEPT. A mutant that changes the output length gives REJECT on `proc.hello/greets` (once each) |
| todo (x86-64 static ELF, no gcc or libc; add/list/clear, files, argv, stderr, exit code) | The correct version ACCEPT. A mutant that drops the newline and a mutant that does not append give REJECT on `add_appends_line`. A mutant that calls fstat gives HOLD (UNSUPPORTED_DURING_RUN) (once each) |
| PIE ELF (the host's /bin/true) | Rejected at load time (exit code 2) |
| Link: sum3 → add64 (x86-64, AArch64) | The correct combination ACCEPT. Making the callee a subtraction gives `link.arith.add64.ensures` (callee); clobbering rbx gives `.abi` (callee); tightening the callee's requires gives `.requires` (caller, `return_to = main+0x18`) |
| Link: copy_twice → copy (region recovery) | The correct version ACCEPT. A callee that copies only half gives `link.mem.copy.ensures`; a callee that reads one extra byte gives a memory violation, with the location `mem+0x…` |

All of the above are fixed in `tests/acceptance.rs`. In the first trial, a correct todo gave REJECT. There were two causes, and Mukoz found both:

1. The program ignored ENOSPC when the file was filled to its limit. The fix was to add a precondition to the contract.
2. Mukoz silently truncated an argc larger than the number of argv expressions. This was a defect in Mukoz and was changed to BINDING_MISMATCH.

## 12.6 native-process and Mach-O (2026-10-05)

### native-process

An Executor that actually runs the subject on the host's CPU and kernel. It runs only x86-64 Linux subjects, on x86-64 Linux hosts (others give `HOST_CANNOT_EXECUTE_TARGET`).

- The subject is started with `execveat` from a sealed memfd. Only the files declared in `[files]` are placed in a temporary directory, and the subject is chrooted into it. User, network, and pid namespaces and rlimits (CPU, memory, file size, number of processes, number of fds) are applied, and if isolation fails the subject is not started (the launch report distinguishes isolation failure from exec failure).
- A raw subject is wrapped in a minimal static ELF and run. Only static ELFs are accepted (including static-pie). An ELF that requests a program interpreter gives `UNRESOLVED_DEPENDENCY`.
- Only stdout, stderr, the exit status, and the declared files are observed. **`machine.memory.access` and `effects.no_forbidden` cannot be observed, so they are NOT_EVALUATED (`REQUIRED_CAPABILITY_UNAVAILABLE`)**. A Suite with only native-process is therefore HOLD; to reach ACCEPT, use a differential test against emulated (`executors = ["emulated", "native-process"]`). SIGSEGV is treated as a memory violation.
- Only subjects permitted by the Policy's trial zone are run (chapter 08).

### Mach-O and `darwin-stdio/1`

- 64-bit Mach-O (arm64) of type MH_EXECUTE. It reads LC_SEGMENT_64, LC_MAIN (called as main, with the return value as the exit code), and LC_UNIXTHREAD. Loading dylibs and chained fixups give `UNRESOLVED_DEPENDENCY`. `[entry] kind = "macho_entry"`.
- System calls put the number in x16 and use `svc #0x80`. exit is 1, read 3, write 4, close 6. A failure sets the carry flag and returns errno. On x86-64 Darwin the number is 0x2000000+n.
- On a Linux host, checking is done with emulated only. native gives `HOST_CANNOT_EXECUTE_TARGET` and NOT_EVALUATED (HOLD). Native execution on an ARM Mac is not implemented (P5).
- The fixture was assembled by hand (`fixtures/process/mkmacho.py`). It has not been checked against files built with Apple's toolchain or signed files `[U]`.

| What | Result (once each, fixed in `tests/native.rs` and `tests/macho.rs`) |
|---|---|
| x86-64 ELF hello and an equivalent hello | ACCEPT in the differential test of emulated + native-process. The output-length mutant gives REJECT on `proc.hello/greets` in both Suites |
| todo (files, argv, exit code) | emulated and native-process agree |
| Mach-O hello (arm64) | ACCEPT on emulated. The mutants (output length, use of x18) give REJECT. native gives HOLD with HOST_CANNOT_EXECUTE_TARGET. With a dylib, UNRESOLVED_DEPENDENCY; with a broken header or command, FORMAT_MISMATCH |
