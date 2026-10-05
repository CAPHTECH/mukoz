# 07 Interface

## 7.1 Policy

- The MVP interface is only a **CLI that outputs JSON**. AI agents, humans, and CI all use the same CLI.
- Every command returns the same JSON envelope (7.3). Human-readable display is produced from the same data with `--format text`. The display side does not hold admission logic.
- JSON-RPC (stdio) and MCP are later extensions that expose the same operations over a different transport (chapter 10).
- The CLI cannot run arbitrary shell, run arbitrary host paths, change Policy, change signing or OS settings, or deploy.
- There is no dependency on an LLM SDK or API key.

## 7.2 Commands

| Command | Description | Runs the subject? |
|---|---|---|
| `mukoz platform probe` | Probe and store host information and the capabilities of each Executor | Only test code for capability checks |
| `mukoz platform qualify --isa <isa>` | Run the engine qualification and store an `EngineQualification` | Test code only |
| `mukoz platform show` | Show the stored host information, capabilities, and qualification records | No |
| `mukoz inspect <file>` | Register a snapshot and return the format-check result and a LoadPlan | No |
| `mukoz check <suite.toml> [--artifact <file>] [--fail-fast]` | Load → plan → run → assess → admission, in one step | Yes |
| `mukoz plan <suite.toml> [--artifact <file>]` | Build and fix a Plan and return its ID | No |
| `mukoz run <plan-id>` | Run a fixed Plan | Yes |
| `mukoz assess <run-id>` | Reissue the admission against the current subject context | No |
| `mukoz show <id> [--page N] [--disasm]` | Partial retrieval of a claim / finding / case / counterexample / trace / diagnostic information | No |
| `mukoz replay <counterexample-id>` | Re-run a counterexample (if the subject has changed, a diagnosis in the new context) | Yes |
| `mukoz shrink <counterexample-id>` | Shrink a counterexample while preserving the property and the premises | Yes |
| `mukoz regressions list <suite.toml>` | List the regression cases for that contract and target | No |
| `mukoz regressions prune <suite.toml> --case <id>…` | Explicitly remove regression cases. The removal is recorded | No |
| `mukoz schema list` / `schema print <id>` | Show the input and output schemas | No |

`check` is the composition of `plan`, `run`, and `assess`, and has no separate verdict logic.

**Implemented commands (2026-10-05):** `check` (`--artifact` `--module <name>=<file>` `--fail-fast` `--gate` `--store` `--policy`. `--module` replaces a module of a link file, chapter 12. `--policy` is the trial-zone Policy; the default is `policy.toml` in the parent directory of the store), `show` (`--page <n>`: long lists in pages of 32 items, with the next command in `_pages`. `--disasm`: disassemble the preceding instructions with Capstone; in a build without Capstone only the display changes), `shrink` (`--budget`, default 400 executions), `replay`, `inspect` (including the load results for ELF and Mach-O), `regressions list` / `prune`, `platform probe` / `show` / `qualify --isa <x86_64|aarch64>`, `expr check`. `plan` / `run` / `assess` / `schema` are not implemented. Running `mukoz` with no arguments prints usage.

**Options:**

- `--artifact <file>`: Override the Suite's `[artifact] path`. Use it when the file name changes on every generation (chapter 04 §4.7).
- `--fail-fast`: Stop at the first counterexample without running the remaining cases. Regression cases run first, so a recurrence is seen immediately. The result is valid as REJECT, but the number of unrun cases is reported (chapter 06 §6.6). ACCEPT_WITHIN_SCOPE requires running all cases, so do not use it when confirming a pass.

## 7.3 Output envelope

```json
{
  "api_version": "mukoz/1",
  "command": "check",
  "ok": true,
  "data": {
    "run_id": "run_…",
    "execution": "completed",
    "assessment": {
      "admission": "ACCEPT_WITHIN_SCOPE",
      "context_match": true,
      "release_authorized": false,
      "scope": {
        "contract": "arith.add64",
        "target": "x86_64/raw/sysv-x86_64/none",
        "platforms": [
          { "executor": "emulated", "host": "linux-x86_64", "engine": "icicle-emu git 3292602fd485 … via mukoz-emu-icicle 0.1.0" }
        ],
        "quantification": "enumerated_cases_not_exhaustive",
        "cases_planned": 4160,
        "cases_completed": 4160,
        "cases_failed": 0,
        "missing_required_claims": 0
      },
      "limitations": ["not_all_bv64_pairs", "not_native_execution"]
    },
    "claims_preview": { "total": 6, "returned": 1, "next_offset": 1, "truncated": true, "items": [] }
  },
  "errors": []
}
```

This is an illustrative shape, not measured output.

- `ok: true` means "the operation completed", not that the check succeeded. Read the admission from `data.assessment.admission`. Even if the subject is REJECT, `ok: true` holds when the check itself finished correctly.
- Output of the subject (stdout, etc.) goes only into fields that are escaped and marked `untrusted` (chapter 08 §8.6).

## 7.4 Exit codes

| Exit code | Meaning |
|---|---|
| 0 | The operation completed (default; unrelated to the admission) |
| 2 | Usage or input error |
| 3 | Internal Mukoz error |
| 4 | Saving the evidence failed |

To turn the admission into an exit code in CI, add `--gate`. Only then are ACCEPT_WITHIN_SCOPE = 0, HOLD = 10, and REJECT = 11 returned. Without `--gate`, the admission and the exit code are not mixed.

## 7.5 Generate-and-check loop

A procedure for the loop in which the AI generates a binary, Mukoz checks it, and the AI reads the result and fixes it.

```text
1. Prepare (human or AI): Write the Contract, Binding, and Suite. Do not write the artifact path in the Binding
2. Generate (AI):         Produce build/add64.bin (outside Mukoz)
3. Check:                 mukoz check suites/add64.toml --artifact build/add64.bin --fail-fast
4. Read the result:       Look at data.assessment.admission
     REJECT  → go to 5
     HOLD    → Read the reason. If it is unsupported or lacking capability, it is a problem of the generation approach or the Suite/Policy. Do not fix it as a fault of the subject
     ACCEPT  → go to 6
5. Diagnose:              mukoz show <finding-id>          failure point, recent instructions, violating access, value mismatch
                          mukoz shrink <counterexample-id>  shrink the counterexample if needed
                          → go back to 2 and fix. The counterexample enters the next check automatically as a regression case
6. Confirm:               mukoz check suites/add64.toml --artifact build/add64.bin   (without --fail-fast)
                          If ACCEPT_WITHIN_SCOPE, it passes within that scope. Read limitations to confirm the scope
```

- Re-run `check` after every fix. Do not skip the verdict by leaving "this binary passed" in the AI's memory.
- The agent needs to hold only the Suite path, the artifact path, the latest run ID, and the IDs of the finding / counterexample it is working on. Do not construct digests by guessing.
- Regression cases are bound to the contract, not the artifact, so they apply automatically to a new binary (chapter 04 §4.8).
- To include native execution in the loop, the owner must configure a trial zone and the isolation capability must be confirmed (chapter 08 §8.4). Without that configuration, the loop runs on emulated only.
- The limits on the number of iterations and on time are set outside Mukoz (on the agent side). Mukoz has limits for each single check.

## 7.6 Limits

| Target | Initial limit |
|---|---|
| Artifact | 64 MiB |
| Expression | depth 64, 8,192 nodes in total |
| Plan | 8,192 cases |
| Guest memory | 16 MiB per case (huge virtual regions are not actually allocated) |
| Detailed trace | 4 MiB per case, 256 MiB per run in total |
| Summary output | target of 8 KiB |
| Page | at most 32 items, 64 KiB |
| stdout/stderr | 1 MiB each per case (can be made smaller in the Suite) |

Limits can be lowered further by Policy. They cannot be raised during a run. If cases remain unrun because of a timeout, the result is HOLD. Unlimited execution is not the default.

## 7.7 Compatibility

- `mukoz/1` is the version of the output envelope. The schema versions of Contract, Binding, Suite, and evidence are kept separately.
- When an output type changes, publish the schema. A breaking change gets a new version.
- The mapping to the 12 operations of the old Mukoz v0.3 that version 0.4 assumed will be decided after checking the old implementation's code and schema (unverified, because it does not exist in this repository).
