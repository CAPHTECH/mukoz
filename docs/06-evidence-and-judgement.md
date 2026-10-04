# 06 Evidence and verdicts

## 6.1 Four categories of checks

| Category | Examples |
|---|---|
| Artifact | Header consistency, entry point scope, dependency resolution |
| Machine | ABI preservation rules, SP, control transfer, memory access, budget |
| Effect | stdout/stderr, exit, attempts at forbidden effects, order of effects |
| Semantic | Addition result, state update, error result, frame |

Semantic checks are observed through a Binding. Passing Machine checks does not substitute for Semantic checks.

## 6.2 Memory conditions

Treat the following as separate conditions.

```text
mapped           the engine can access it
owned            by contract, whose region it is
initialized      the initial value is determined
allowed_read     may be read
allowed_write    may be written during execution
unchanged_at_end must equal its value at the start when execution ends
```

- To forbid writing to an out-of-scope region and then restoring it, a comparison at the end is not enough. Monitoring of all writes is required.
- Check that `[addr, addr+width)`, including the access width, fits entirely inside an allowed region. Also handle overflow in address addition and accesses that cross regions.
- If the contract forbids reading uninitialized regions, detect a violation as a policy violation. If any initial value is allowed, vary that value as an input. Do not treat success with a random initial value as success for all initial values.

## 6.3 Observing and enforcing effects

For each capability, keep **whether it can be observed** and **whether it can be forcibly stopped** as separate values.

| Capability | emulated (routine) | emulated (process + effect model) | native-routine | native-process |
|---|---|---|---|---|
| Registers | Observe | Observe | Observe at entry and exit only | Not possible |
| Memory access | Observe and enforce all accesses | Same as left | Page-level enforcement only | Not possible |
| stdout/stderr | Effect is forbidden | Virtual byte string | Effect is not possible | Real byte string |
| Syscall attempts | Observe and enforce | Model the allowed ones, stop on others | Initially not possible | Initially not possible |
| Absence of communication | Confirm inside the model | Stop anything not allowed | Cannot be asserted | Cannot be asserted (enforcement only, if an isolation capability exists) |
| Host protection | Not guaranteed by worker separation alone | Same as left | Same as left | Isolation capability required (chapter 08) |

"Must not attempt communication" and "communication does not leave the machine" are different properties. Even if isolation stops the send, a violation of the former can still occur. Also, stopping an effect through isolation can change the subject's behavior, so do not generalize a result from an isolated environment to an unrestricted environment.

## 6.4 Claim axes

Do not collapse a result into a single PASS. Keep the following for each property.

| Axis | Values | Meaning |
|---|---|---|
| `evaluation` | `SATISFIED_IN_SCOPE` `VIOLATED` `INCONCLUSIVE` `NOT_EVALUATED` | What was learned about that property |
| `method` | `structure` `example` `property` `differential` | Checking method |
| `execution` | `completed` `budget_exhausted` `worker_failed` `blocked` `unsupported` | How the execution ended |
| `platform` | Executor, host, engine (chapter 03 §3.5) | Where the result was obtained |
| `independence` | `independent` `previous_version` `self` | Relation between the checker and the subject (§6.9) |
| `context_match` | `true` / `false` | Whether it applies to the current subject context |

Values for formal verification (`exhaustive_within_domain`, `solver_checked`, etc.) are reserved in the data model but are not output until they are implemented.

## 6.5 Gaps in monitoring and trace

```text
monitoring_complete      whether the monitoring needed for the verdict ran to the end
trace_storage_truncated  whether the stored detailed trace has omissions
response_truncated       whether this response is only partial
```

Omitting the display trace does not break the verdict if all monitoring was performed. If a monitoring event itself was dropped, make the affected claim INCONCLUSIVE.

## 6.6 Admission (Assessment)

```text
if subject_context does not match the current one:
    HOLD (STALE_CONTEXT)
else if there is a counterexample to a required claim that is valid in the current context:
    REJECT
else if a required claim has NOT_EVALUATED / INCONCLUSIVE / missing data / incomplete execution:
    HOLD
else if the platform scope required by the Policy is not met:
    HOLD (PLATFORM_SCOPE_UNMET)
else if there are 0 valid cases, or a required situation was not reached:
    HOLD (VACUOUS_SCOPE)
else:
    ACCEPT_WITHIN_SCOPE
```

- A worker failure in a later case never erases a counterexample already obtained.
- REJECT is also valid when `--fail-fast` (chapter 07) did not run the remaining cases after the first counterexample. REJECT needs only one counterexample valid in the current context. However, report the number of unexecuted cases in `scope`, and do not write "the other properties were satisfied".
- Do not use a counterexample from an old artifact for REJECT of a new artifact. Reproduce it in a new run.
- A client that receives an unknown claim kind or evaluation value treats it as HOLD (it does not guess PASS).
- `release_authorized` is always `false`.

## 6.7 Timeouts and abnormal termination

| Situation | Handling |
|---|---|
| The execution budget for the check was used up | INCONCLUSIVE → HOLD |
| The "within N instructions" written in the contract was exceeded | VIOLATED for that property |
| The wall-clock limit written in the contract was exceeded | A violation under that measurement condition. State the measurement error and reproducibility alongside |
| The worker crashed | `WORKER_FAILED`. Not equated with an anomaly in the subject |
| The subject stopped on a forbidden memory access | A counterexample if it is a forbidden action under a valid initial state and a corresponding model |

Not returning within a set time under a `must_return` contract is not taken as a mathematical proof of non-termination.

## 6.8 Differential test

Run the same case on multiple Executors and hosts and compare the observations.

| Combination | What it yields |
|---|---|
| `emulated` and `native-routine` (same-ISA host) | Differences in instruction semantics between the emulator and the real CPU |
| `emulated` and `native-process` | Differences between the effect model and the real OS |
| `emulated` on different hosts | Differences due to host and engine build |
| The Contract's reference computation and the subject | The check proper |

- Agreement does not prove the correctness of either side.
- Unicorn derives from QEMU, so do not treat agreement between Unicorn and QEMU (qemu-user) as agreement between two independent implementations.
- The cause of a mismatch (subject, Binding, ABI, effect model, engine) is unknown, so first return `BACKEND_DIVERGENCE` / `PLATFORM_DIVERGENCE` together with whatever evidence can be examined.

## 6.9 Checker independence

To handle self-check (chapter 09 §9.6), claims carry `independence`.

| Value | Meaning |
|---|---|
| `independent` | The expression evaluator and engine used for the verdict are separate implementations from the subject |
| `previous_version` | The checker comes from the same source lineage as the subject, but is a different (earlier) version |
| `self` | The checker and the subject are built from the same source and the same version |

- Issuing ACCEPT_WITHIN_SCOPE from `self` claims alone is allowed only when the Policy states so explicitly. By default, a required claim that is `self` only results in HOLD.
- Implementation (2026-10-05): `assessment.independence` outputs `value`, `basis` and `checker_sha256`. `self` / `previous_version` is determined only when the Suite has `[selfcheck] checkers = "…/checkers.toml"` (schema `mukoz.checkers/1`, `previous = [{ version, sha256 }]`). If the digest of the running mukoz is listed, it is `previous_version`. A run that would be ACCEPT with `self` becomes HOLD `SELF_CHECK_ONLY`. Policy `allow_self_accept = true` allows it explicitly.
- When the expression evaluator itself is the subject, computing expected values with that evaluator is circular. Take expected values from one of: the instruction results of a real CPU (native-routine / emulated), a table of hand-determined values, or a separate implementation.

## 6.10 Counterexamples, shrinking and re-execution

A counterexample has at least the following.

```text
IDs of artifact / subject context / plan / case
property ID and failure predicate
concrete initial registers, memory and inputs
script of environment responses
stop reason, PC, recent trace, effects
expected / observed
execution_platform
search range of shrinking and its completion state
```

- When shrinking, make the input values, buffers, input sequences and environment script smaller. Do not rewrite the Contract, the meaning of the Binding or the expected values to make the failure go away. Keep the precondition and the failure predicate.
- The initial shrinking implementation is a simple search that moves each input toward 0, boundary values and short byte strings.
- That a counterexample did not reproduce on the fixed artifact means "the regression test for this counterexample passed". It does not mean overall correctness.

## 6.11 Storing evidence

```text
.mukoz/
  objects/sha256/…     # immutable: artifact, contract, binding, suite, plan, run, claim
  host/…               # HostProbe, EngineQualification
  indexes/…            # derived indexes. Can be rebuilt from objects if lost
  work/…               # worker scratch area
policy.toml            # policy written by the owner (optional)
```

- No SQL server or distributed storage is needed. Content-addressed files are the source of truth, and indexes are derived.
- Create files by writing to a temporary file and replacing it with rename, so that a crash midway leaves no corrupted objects. The semantics of rename and locking differ by OS, so test the Store implementation per OS (chapter 09).
- Name objects only by a lowercase hex digest, so that names do not collide even on case-insensitive filesystems (the default on macOS and Windows).
- Evidence has no cryptographic tamper resistance. A process running with the same user's privileges can rewrite `.mukoz/` (chapter 08).
- Do not store the full guest memory of every case. Keep the size down by sharing immutable pages and recording initial data plus changed pages. However, always make it possible to restore the concrete values needed for re-execution.
- If evidence capacity runs short, do not return success after losing counterexamples or verdict material.

## 6.12 Diagnostics for fixing

For an AI to fix machine code, "what happened at which instruction" is the most useful information. Attach **diagnostics**, separate from the verdict, to findings and counterexamples.

| Item | Content | How obtained |
|---|---|---|
| Failure point | The PC at the stop, the offset in the artifact, the stop reason (forbidden access, unsupported instruction, budget exhausted, return-target violation, etc.) | Recorded by monitoring |
| Recently executed instructions | Up to 64 instructions executed before the stop. Address, offset, instruction bytes, disassembly | A small PC ring buffer (64 entries) that is always running. Even for variable-length instructions such as x86_64, it disassembles from the executed PCs, so instruction boundaries are not wrong |
| Violating access | Address, width, read or write, which region it was outside of, list of allowed regions | Memory monitoring |
| Register differences | Registers whose values changed between entry and exit (or at the stop). If it is an ABI violation, which register | Snapshots at entry and exit |
| Value mismatch | Property ID, the expression evaluated, the value of each subexpression (expected/observed). Example: `result.value = 0x…`, `input.a + input.b = 0x…` | The expression evaluator keeps the subexpression values |
| Path taken | If the return value is wrong (execution did not stop), the list of instructions executed in that case (duplicates are collapsed into counts, up to 256 instructions) | Execution trace. If the trace budget is exceeded, omit it and indicate the omission |
| First divergence point of a differential test | If results differ between Executors, the first instruction where the states diverged (when obtainable) | Both traces |

- Diagnostics are **not used for the verdict**. Capstone is used for disassembly, but it is for display and is not the basis for the meaning of instructions. In an environment where Capstone is not available, omit the disassembly column and state why it was omitted. Always output the instruction bytes.
- The size of diagnostics has a limit per finding (default 16 KiB). If exceeded, retrieve it in stages with `show`.
- `mukoz show <finding-id> --disasm` also gets a static disassembly, taken from the artifact, of the code around the failure point. This is not based on executed PCs, so the display states clearly that instruction boundaries may be wrong.

## 6.13 Projections for AI and humans

- The default output is only a small summary and the top findings.
- Details are retrieved in stages by specifying run / case / property / PC / event kind. Pages carry `total / returned / next_offset / truncated`.
- Return stable IDs for following a counterexample. This removes the need to paste old, huge logs back into the conversation.
