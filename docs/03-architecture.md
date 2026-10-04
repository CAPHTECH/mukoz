# 03 Architecture and data model

## 3.1 Logical structure

```text
   Contract / Binding / Suite (TOML or JSON)        Artifact file
                  │                                      │
                  ▼                                      ▼
            Spec Loader ──────────────►  Artifact Store + Inspector
                  │                                      │
   Policy ────────┤          HostProbe / Qualification ──┤
                  ▼                                      ▼
            Planner (capability matching, case generation, freezing)
                                   │
                              Frozen Plan
                                   │
                              Supervisor
            ┌──────────────┬───────┴───────┬──────────────────┐
            ▼              ▼               ▼                  ▼
     emulated worker  native-routine  native-process   translated-process
     (Unicorn)        worker          worker           worker
            └──────────────┴───────┬───────┴──────────────────┘
                                   ▼
                     Observation → Evidence Store
                                   │
                                   ▼
                     Assertion Evaluator → Assessor
                                   │
                                   ▼
                 Claims / Counterexamples / Assessment
```

## 3.2 Components

| Component | Responsibility | Responsibilities it is not given |
|---|---|---|
| Spec Loader | Strict loading of TOML/JSON, parsing and type checking of expressions, normalization, digest | Deciding expected values from natural language |
| Artifact Store | Immutable snapshots, digests, derivation relations (before and after signing, etc.) | Automatically assigning trust labels |
| Inspector | Per-format structural checks, entry point resolution, dependency detection, LoadPlan creation | Fully standing in for the OS loader |
| Binding Validator | Consistency of entry point, registers, regions, and ABI | Admitting that a Binding is semantically correct |
| Host Probe | Host information, Executor capability checks, recording of engine qualification | Guessing capabilities, adopting declared values |
| Planner | Capability matching, case generation, budget, freezing the Plan | Silently dropping required claims that cannot be checked |
| Supervisor | Supervising worker launch, deadlines, resources, and crashes | Granting shell privileges to the subject |
| Worker | Actually runs on one Executor and returns observations | Changing the contract or verdict criteria |
| Effect Model | Effect semantics (write/exit, etc.) and per-OS x ISA syscall adapters | Forwarding unknown syscalls to the host |
| Assertion Evaluator | Deterministic evaluation of typed properties from observations | Pass/fail by fuzzy scoring |
| Assessor | Admission based on scope, missing measurements, counterexamples, and context | Release approval |
| Evidence Store | Immutable evidence, index, paged projection, reproduction material | Unconditional reuse of old results |

## 3.3 Process separation

- Do not load the subject's code into the Mukoz main process (no `dlopen` and no pointer calls).
- The emulator FFI (Unicorn) is also placed in a separate-process worker. If the worker crashes, the main process continues to produce verdicts and records `WORKER_FAILED`.
- Worker separation is a boundary for crashes and resources; it is not a sandbox that protects the host from malicious code (chapter 08).
- The main process and workers communicate over versioned structured IPC. The handshake exchanges the worker build ID, Executor, and capabilities, and the run/case ID, event order, and payload length are checked. The subject's stdout is not mixed into the IPC.

Monitoring of prohibitions such as memory accesses may be evaluated incrementally inside the worker for performance. That monitoring code is part of the trusted base. Even when the saved trace is omitted, whether the monitoring needed for the verdict continued to the end is recorded separately (chapter 06 §6.5).

## 3.4 Main objects

| Object | Main fields |
|---|---|
| `ArtifactSnapshot` | digest, byte length, format, ISA, selected slice, dependencies, origin, parent artifact |
| `Contract` | schema version, boundary (routine/process), types, property IDs, normalized AST, digest |
| `Binding` | contract ID, target platform, entry point, correspondence of arguments, results, and regions, effect model, digest |
| `Suite` | contract/binding ID, generator, seed, number of cases, budget, Executor, required claims |
| `Policy` | Permitted Executors and hosts, required claims, limits, digests of subjects permitted for native execution, trial zone (chapter 08 §8.4) |
| `HostProbe` | host ID, OS version, CPU and features, checked capabilities per Executor, probe version |
| `EngineQualification` | engine name and build, ISA, host, version and result of the qualification test |
| `Plan` | subject context, case set (concrete values of regression cases and generated cases), generator version, Executor, applied limits |
| `Run` | plan ID, execution platform, state, observations per case, termination reason |
| `Claim` | property ID, evaluation, method, scope, assumptions, limits, independence, evidence references |
| `Finding` | violated property, expected/observed, location, related trace |
| `Counterexample` | input, initial state, and environment responses for reproduction, subject digest, failure predicate |
| `RegressionCase` | Input of a past counterexample tied to the contract digest and target. Not tied to the artifact (chapter 04 §4.8) |
| `Assessment` | admission, incomplete items, applied platform scope, `release_authorized = false` |

## 3.5 Identity: subject context and execution platform

0.4 put everything into one context digest. Including host information makes every result from another host a mismatch; excluding it leaves host differences unrecorded. So it is split in two.

```text
subject_context = H(
  artifact_snapshot, selected_slice,
  contract, binding, suite_semantics, environment_model,
  policy_revision, evaluator_version
)

execution_platform = {
  host_id, os_version, cpu_model, cpu_features,
  executor, engine_name, engine_build, worker_build,
  translator_name_and_version   # translated-process only
}
```

**Rules:**

1. As a precondition of admission, the evidence's `subject_context` must match the current value. A mismatch is HOLD (stale evidence).
2. `execution_platform` is treated as the **scope of the claim, not a matching key**. A claim must always carry which Executor and which host it was obtained on.
3. A Policy can specify the required platform scope (for example, "at least one `native-process` result on `linux-x86_64`"). If it is not met, HOLD.
4. A result from `emulated` is adopted only while the `EngineQualification` of that host and engine build is valid. A result from a host with no qualification record is HOLD.
5. Results obtained on different hosts are not silently merged into one. If the results disagree, `PLATFORM_DIVERGENCE` is recorded (chapter 06 §6.8).

A Plan has a separate content ID derived from the `subject_context` and the case set. If the seed, generator version, or case order changes, it is a different Plan.

The real address after ASLR is per-run information and is not used as a persistent identifier. Locations are expressed as `artifact + slice + file offset / image-relative position`.

## 3.6 Signing and distributed artifacts

Before and after signing (Mach-O code signing, PE Authenticode, etc.), the file is a different `ArtifactSnapshot`. Even if `.text` is the same, results for the whole file are not reused. This is because behavior can change if the entry point, load information, data, or dependencies change. Signing is done by the owner outside Mukoz, and the signed file is newly registered.

## 3.7 Representation of values

- Do not put 64-bit register values or addresses in a JSON number. A bv value is a fixed-width lowercase hex string whose width follows the bit width (`"0x00000000000000ff"`).
- For stdout/stderr, the byte sequence is the source of truth. UTF-8 is a projection for display; invalid UTF-8, NUL, and newlines are not normalized.
- Sizes and counts are integers with a defined upper bound.
