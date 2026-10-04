# 09 Implementation plan and verification

## 9.1 Language and approach

- The implementation language is **Rust**. The reason is ADR-07 in chapter 10.
- Start from a small core. Do not build a large set of microservices or a dynamic plugin ABI.
- Host-dependent code stays inside `mukoz-platform` and the workers. Core does not depend on the host OS or CPU.
- The first completion target is **P1 through P4 passing on the current environment, `linux-x86_64` (Debian 13)**.

## 9.2 Repository layout (proposal)

```text
mukoz/
  Cargo.toml                  # workspace
  crates/
    mukoz-kernels/            # #![no_std]. Pure operations such as bv arithmetic and range checks. Used by both Core and the self-check
    mukoz-core/               # types, expressions (parsing, type checking, evaluation), Contract/Binding/Suite, plan, admission
    mukoz-artifact/           # snapshot, raw/ELF/Mach-O/PE inspection, LoadPlan
    mukoz-platform/           # ABI table and syscall adapter table (data), HostProbe, capability model
    mukoz-store/              # content-addressed store, index, projection
    mukoz-proto/              # worker IPC (versioned)
    mukoz-worker-emu/         # bin. Unicorn (the only place with FFI)
    mukoz-worker-native/      # bin. native-routine / native-process (cfg per OS)
    mukoz-cli/                # bin `mukoz`
  fixtures/
    <name>/{src.s, <isa>.bin, manifest.toml}   # original assembly, generated output, digest and toolchain
  conformance/
    x86_64/ aarch64/          # engine qualification (instruction tests with known answers)
  selfcheck/
    contracts/ suites/ checkers.toml          # for the self-check (9.6)
  schemas/
  tests/
  docs/
```

## 9.3 Dependency candidates

| Region | Candidate | Points to confirm |
|---|---|---|
| Format reading | `object` | Reading API for ELF/Mach-O/PE. Consistency checks are written in-house |
| CPU emulation | Unicorn 2 + Rust binding (`unicorn-engine`) | Build on this host `[U]` (there is no system library; the assumed approach is to build the bundled source with cmake), license (chapter 10) |
| Disassembly | Capstone + `capstone` crate | For display. Not on the required path |
| Structured I/O | `serde`, `serde_json`, `toml` | Reject unknown fields, reject duplicate keys, bounded reads |
| digest | `sha2` | Computed over canonical JSON |
| Random numbers | **A small in-house PRNG** (e.g., SplitMix64) | The generated sequence can change when an external crate's version changes `[R]`. Record the generator version in the evidence |
| Linux process control | `rustix` etc. | memfd, execveat, cgroup, namespace |
| Windows | `windows-sys` | Later |

When adopting a dependency, pin its version (or commit). Do not track `latest`.

## 9.4 Stages

P0 through P5 below describe groupings of the features to build. Development built P1 and part of P3 (regions and pointers) first, and ran a comparison trial of the value hypothesis (agents with and without Mukoz) before the Tier 1 acceptance for `linux-x86_64` (§9.8).

| Stage | What to build | Completion condition (all on `linux-x86_64`) |
|---|---|---|
| **P0 Core** | kernels, expression language, TOML/JSON loading, Store, Assessor, platform table, `platform probe` (host information only) | Known-value tests for expression evaluation, unknown-field rejection, STALE/UNKNOWN→HOLD, and VACUOUS_SCOPE tests pass |
| **P1-0 Spike** | Throwaway code that builds the Unicorn Rust binding and runs one x86_64 add64 | The build succeeds and one execution works. If not, consider the alternatives in chapter 10 before going further |
| **P1 x86_64 routine** | emulated worker, x86_64 SysV harness, sentinel, memory monitor, planning and generation, `check` (`--artifact`, `--fail-fast`), regression cases, diagnostics (PC ring buffer, value mismatch, violating access), x86_64 engine qualification, native-routine worker, differential test | The first vertical slice in §9.5 passes |
| **P2 aarch64 routine** | aarch64 harness (AAPCS64), aarch64 engine qualification | add64 / sub mutant / nested call can be checked with the **same Contract** |
| **P3 process** | ELF Inspector, `linux-stdio/1` (x86_64 and aarch64 adapters), Linux native-process, Linux isolation capability probe and trial zone, Mach-O Inspector, `darwin-stdio/1` | The x86_64 ELF hello gets ACCEPT in both emulated and native-process, and the mutant gets REJECT. The Mach-O hello can be checked in emulated, and native gets HOLD with `HOST_CANNOT_EXECUTE_TARGET` |
| **P4 self-check and finishing** | `replay`, `shrink`, `show` pages and `--disasm` (Capstone), the `regressions` command, self-check stages 1 to 3 (§9.6) | The acceptance criteria in §9.8 are met → `linux-x86_64` becomes Tier 1 |
| P5 other hosts | macOS arm64 → Linux arm64 → Windows x86_64 (PE, win64, API stub model) | Tier 2 → Tier 1 per host |
| Later | JSON-RPC, fault injection scripts, bounded verification | Chapter 10 |

P1-0 comes first because building and running Unicorn is the largest uncertainty in this plan.

## 9.5 First vertical slice

```text
fixtures/add64 (x86_64: 48 8d 04 37 c3)
  + arith.add64 Contract
  + arith.add64@x86_64-sysv Binding
  + boundary values Cartesian product + random 4096
        ↓
mukoz check
        ↓
Correct implementation:      ACCEPT_WITHIN_SCOPE
sub mutant:                  REJECT + arith.add64/sum + counterexample + diagnostics
                             → the counterexample is saved as a regression case
Recheck the sub mutant:      with --fail-fast, regression case reg-… fails first
Restore the correct impl.:   ACCEPT_WITHIN_SCOPE, including the regression case
With unsupported instruction: HOLD + UNSUPPORTED_DURING_RUN
native-routine available:    agrees with emulated on the same cases
        ↓
mukoz show / mukoz replay
```

This stage does not include ELF, Mach-O, processes, or the self-check at the same time.

## 9.6 Self-check

Check Mukoz's executable (an x86_64 ELF on Linux) and its components with Mukoz itself. The purpose is to have a **net that catches regressions and platform differences**. It is not a proof of Mukoz's correctness.

### Stages

| Stage | What is checked | Executor | When |
|---|---|---|---|
| 1 | Mukoz's verdicts on fixtures with known answers and on mutant fixtures (§9.7) | all | From P0 |
| 2 | The `mukoz` CLI, checked with a contract at the process boundary (stdout JSON and exit code for an input file) | native-process | P3 onward |
| 3 | `mukoz-kernels` functions, checked as routines through `extern "C"` entry points. Built for both x86_64 and aarch64 | emulated, native-routine | P4 |
| 4 | Emulation of whole processes that use the Rust standard library | — | Not for now. It would need a broad Linux syscall model |

### Stage 3 subjects and the source of expected values

To avoid circularity, decide the source of expected values per layer.

| Layer | Example | Source of expected values |
|---|---|---|
| Bottom layer: bv arithmetic | `mk_bv_add64`, comparison, shift, sign extension | Instruction results on a real CPU (running the same arithmetic instruction in native-routine) and a table of values fixed by hand. Mukoz's expression evaluator is not used |
| Middle layer: range checks | `mk_range_contains(base, size, addr, width)` (the core of the memory monitor) | A contract written in the expression language. The expression evaluator depends on the bottom-layer operations that have already been checked |
| Upper layer: structure checks | The ELF header check function (takes a buffer) | A table of correct and malformed headers built by hand |

- kernels are built with `#![no_std]`, `panic = "abort"`, no memory allocation, and `#[no_mangle] extern "C"`.
- If the compiler inserts calls such as `memcpy` or `memset`, the function contains external calls, so routine checking gives HOLD with `UNRESOLVED_DEPENDENCY`. Detecting this correctly is also a test item.
- Comparing a checker and a subject built from the same source mainly finds differences due to the compiler, optimization level, and ISA. Logic errors are caught by the sources of expected values above.

### Checker versions

- `selfcheck/checkers.toml` records the digest of the previous version of the `mukoz` executable used for checking. The self-check of version N requires the result obtained with the version N-1 checker. The result from version N itself is recorded additionally as `independence = self` (chapter 06 §6.9).
- The first version (which has no N-1) relies only on the known-answer tests of stage 1 and the bottom-layer comparison against a real CPU.

## 9.7 Mukoz's own tests

### fixtures

| fixture | ISA | What it confirms |
|---|---|---|
| add64 / sub64 | x86_64, aarch64 | width, arithmetic, result Binding |
| signed comparison / unsigned comparison | both | the comparison signedness is not confused |
| checked increment | both | pinned entry pointer, state update, failure paths, frame |
| bounded copy | both | region, access width, boundary, alias |
| nested call | both | does not stop at the first `ret` |
| scratch stack / red zone | x86_64 | a legitimate temporary write is not mistaken for a forbidden effect |
| callee-saved clobber | both | ABI claim |
| x18 temporary use | aarch64 | A violation under `apple-arm64`. Its handling under `aapcs64` (Linux) will be decided after the ABI table is confirmed `[U]` |
| hello ELF (no libc) | x86_64 | entry resolution, virtual stdout, exit, agreement with native |
| equivalent hello | x86_64 | accepts a different instruction sequence with the same output |
| hello Mach-O | aarch64 | `LC_MAIN`, `darwin-stdio/1` |
| broken ELF / Mach-O | — | header, command, and segment boundaries; integer overflow |
| dynamically linked ELF | x86_64 | `UNRESOLVED_DEPENDENCY` is not treated as a functional violation |
| unsupported instruction | both | no NOP replacement or fallback |
| infinite loop | both | distinguishes budget exhaustion from a termination violation |

Each fixture keeps its original assembly, generated output, digest, and toolchain version in `manifest.toml`.

**Fixture creation (state as of 2026-10-05):** aarch64 fixtures are built with Rust's `aarch64-unknown-linux-gnu` target (`global_asm`) and `llvm-objcopy` (`fixtures/aarch64/build.py`). The Mach-O fixture is assembled by hand (`fixtures/process/mkmacho.py`). What follows is the record from when this document was written.

**Constraints on fixture creation (when this document was written):**

- x86_64 can be built with `as` / `ld` (hello was built and run once).
- There is no aarch64 assembler or disassembler. Either install `binutils-aarch64-linux-gnu` (apt, to be confirmed) or add Rust's `aarch64-unknown-linux-gnu` target. Neither has been done.
- There is no Apple toolchain to build a Mach-O hello. Either obtain `hello-arm64` from 0.4 or assemble it by hand (chapter 10, open items).
- Values recorded for `hello-arm64` in 0.4 (not rechecked when this document was written; the file is also not in this directory):

  | Item | Before signing (confirmed when 0.4 was made) | After signing (user report only) |
  |---|---|---|
  | Size | 16,384 bytes | — |
  | SHA-256 | `a10c4e12b5e8d1f62b649ed16d2384e04db7081b03b6e18dea2ce547d1c418cc` | `fe607f74d161462411391222959671488698f5b09952e9570f6ddbf8bac71e67` |
  | header | ncmds = 6, sizeofcmds = 376 | ncmds = 7, sizeofcmds = 392 |
  | Instructions | file offset `0x300`, 32 bytes (8 instructions; does not check the return value of write) | — |
  | Message | file offset `0x320`, 29 bytes (`Hello from raw ARM64 Mach-O!` plus a newline, inferred `[R]`) | — |

  It has been reported that the program was launched on an ARM Mac and the string was displayed, but this was not an automated test that captured the stdout bytes and exit status.

### Mutations

Make mutations with a clear meaning from correct fixtures, and decide first **which property of which contract each one violates**.

```text
add → sub, 64-bit → 32-bit, signed branch → unsigned branch
write 1 byte outside the permitted range, write outside the target and then restore it
clobber callee-saved, use a reserved register temporarily and restore it
return to an invalid return address, change the stdout length by 1 byte
ignore a write failure (not a counterexample in a full-success environment; examine it in a separate suite with fault injection)
break the entry or a load command, attempt an unneeded external effect
```

### Negative tests for the checker

| Subject | Test |
|---|---|
| Artifact | change after inspect, change by signing, a different file with the same name |
| Binding | entry offset shift, result register shift, wrong width, stale digest |
| Plan | zero cases, all cases outside the precondition, bypassing a required claim, insufficient budget |
| Assessor | wrong conversion of UNKNOWN→PASS, a counterexample lost due to a later failure, a stale REJECT |
| Platform | an emulated result on a host without a qualification record, HOST_CANNOT_EXECUTE_TARGET, automatic fallback between Executors |
| Evidence | missing trace, omitted projection, change to the replay target, crash during a write |
| Engine | leakage of registers, memory, effects, or cache from the previous case |
| CLI | unknown fields, oversized input, exit code with and without `--gate`, neither `--artifact` nor `[artifact]` present |
| Regression case | an old set becomes "not applicable" after a contract change, a case that cannot be applied after a Binding change becomes NOT_EVALUATED, the plan is rejected when the limit is exceeded, `include = false` appears in limitations |
| fail-fast | a run cut off after a counterexample does not become ACCEPT, the number of unexecuted cases is shown |
| Trial zone | if one isolation capability is missing, `NATIVE_NOT_PERMITTED`; no switch to emulated; a file pointing outside the zone through a symbolic link is rejected |
| Diagnostics | admission does not change without Capstone, verdict does not change when the diagnostics limit is exceeded |
| Security | false PASS through the subject's output, path manipulation, log amplification |

## 9.8 Acceptance criteria (conditions for making `linux-x86_64` Tier 1)

These are criteria to be met in the future, not current measured values.

1. The correct fixtures in §9.7 get ACCEPT_WITHIN_SCOPE with the corresponding Executor and case set.
2. Mutants within the supported scope are REJECTed with the announced property ID, and the counterexample can be replayed.
3. A change to the file, signature, or contract does not allow an old admission to be reused.
4. There is no path that turns unsupported, zero cases, timeout, or missing measurement into success (confirmed by negative tests).
5. Rerunning cases with the same subject context and the same execution platform gives the same result. If they differ, the reason is recorded.
6. No guarantee about memory or communication that was not observed is attached to a native-process result.
7. An AI agent can step from the summary down to the finding, the counterexample, and the needed trace.
8. The differential test between emulated and native-routine on x86_64 agrees within the scope of engine qualification.
9. Self-check stages 1 to 3 pass, and `independence` is recorded correctly.
10. **Generation loop test:** an AI agent, using only Mukoz's CLI output as a clue (without reading Mukoz's internal files or the fixtures' correct answers), can fix a mutant fixture and get from REJECT to ACCEPT_WITHIN_SCOPE. Record the number of iterations to get there and the number of times a regression case caught a recurrence along the way.

"Zero false acceptances" is a goal within the fixture set. It is not a guarantee about unknown binaries in general.

**Achievement status (2026-10-05, working host `linux-x86_64`, once each):** `tools/tier1.py` runs the tests and runs decided per criterion and writes `target/tier1-report.json`.

| Criterion | Basis | Result |
|---|---|---|
| 1, 2 | `tests/acceptance.rs`, `tests/coverage.rs` (fixture tables for both ISAs), `tests/native.rs`, `tests/macho.rs`, shrink → replay | Passed |
| 3 | `tests/negative.rs`: a change to the artifact, a different file with the same name, the contract, the Binding, or the Suite changes the subject context, and the old regression set is counted as "not applicable" | Passed |
| 4 | I1-series tests; admission is the same for zero cases, timeout, a native process that does not stop, broken evidence, limit exceeded, and a build without Capstone | Passed |
| 5 | Rerunning the same subject gives the same admission, claims, and counterexample inputs and observations (only the run / counterexample IDs differ); a standalone replay of a counterexample gives the same observations as the batch run; regression case IDs and seeds are the same across runs | Passed |
| 6 | In a run with native-process only, memory and effects are NOT_EVALUATED | Passed |
| 7 | `tests/navigation.rs` | Passed |
| 8 | 4160 add64 differential test cases agree, a cpuid mismatch is detected, engine qualification (x86_64: 45 tests, 533 vectors, also compared against a real CPU) | Passed |
| 9 | `selfcheck/run.py`: stage 1 (cargo test), stage 2 (static mukoz CLI under native-process, a table of 11 rows), stage 3 (7 kernels functions × 2 ISAs, x86_64 compared with native-routine). Because `independence = self` (there is no previous-version checker), stages 2 and 3 are HOLD `SELF_CHECK_ONLY`, and all claims were satisfied | Passed (HOLD is by design) |
| 10 | `selfcheck/genloop/2026-10-05/`: a separate agent fixed the 6 defects in todo using only CLI output, going from REJECT to ACCEPT in 8 checks. The number of times a regression case caught a recurrence is 0 (no recurrence happened) | Recorded |

Unverified or not met: continuous execution (CI) is not set up. Runs of stages 2 and 3 with `previous_version` have not been done because no previous-version checker exists (`selfcheck/checkers.toml` is empty). The generation loop test was run once, on one task.

## 9.9 Measurement

Measure the detection rate for known defects, false rejection of correct fixtures, the uncheckable rate, the counterexample reproduction rate, the number of operations to reach a diagnosis, the number of bytes returned, CPU time, peak memory, and the number of AI iterations to a fix. For mutants excluded as unsupported, state the count and the reason. Performance target values will be decided after the first baseline measurement.
