# 11. Usage for agents

The procedure an AI agent follows to generate or fix a binary without source and to check with Mukoz whether it satisfies its contract. The implemented scope is in chapter 04 §4.10 and chapter 07 §7.2. If they disagree, the implementation is correct.

## 11.1 The generate-and-fix loop

```text
1. Read contract.toml             Everything that can be checked (ensures, frame, requires) is here
2. Build / fix the binary
3. mukoz check suite.toml --artifact <file> --store .mukoz
4. Read data.assessment.admission
   ACCEPT_WITHIN_SCOPE → done (the scope is written in scope and limitations)
   REJECT             → go to §11.2
   HOLD               → go to §11.3
5. Go back to 2
```

- The output is always `{api_version, command, ok, data, errors[{code,message}]}`. **For usage or input errors (missing file, contract type error, etc.) the output is `ok: false`, `data: null`**, and the reason is in `errors`. In that case nothing is run and nothing is added to the store. Exit codes are 0 (a verdict was produced), 2 (usage or input error), 4 (the store cannot be opened). With `--gate`: ACCEPT_WITHIN_SCOPE=0, HOLD=10, REJECT=11.
- Use `--fail-fast` to iterate quickly. Do not use it when confirming a pass (with it the admission never becomes ACCEPT).
- Earlier counterexamples are run first, automatically, as regression cases (keep using the same `--store`).

## 11.2 Reading a REJECT

| Where to look | Content |
|---|---|
| `data.assessment.reasons` | The list of violated properties (`VIOLATED: <property ID>`) |
| `data.findings[]` | One entry per violated property (the first counterexample of that property). Inputs (`inputs`), stop reason (`stop`), observed values, the last 16 instructions (offsets and bytes), `detail`. Reach the other counterexamples with `mukoz show` from the IDs in the claim's `counterexamples` |
| `case_id` is `reg-…` | A regression case built from a counterexample of a previous run. The ID is the same across runs, so you can follow the same case |
| `detail.why_false` | **Read this first.** Why the expression became false: the index of the first counterexample of a `forall` (`witness`), the values of both sides of the false comparison (observed and expected), the index values of index accesses. For `and`, only the false side is traced |
| `recent_instructions` | The last 16 instructions at the point of the stop. For memory violations and illegal instructions, it points at that location. **For a wrong result (ensures), only the instruction just before the return appears**, so narrow down the location from `why_false` and the inputs |
| `mukoz show <counterexample ID> --store .mukoz` | All information about the counterexample |
| `mukoz replay <counterexample ID> --artifact <file> --store .mukoz` | Re-run only that counterexample on the fixed binary. `property_now` shows how it stands now |

Kinds of property, and where to suspect first:

| Property | Meaning | Where to suspect first |
|---|---|---|
| `<contract>/<ensures ID>` | The result does not satisfy the contract's expression | Compare subexpression values with the inputs |
| `<contract>/frame.<state>` | State that must not change did change | The extent of writes |
| `machine.memory.access` | Access outside the permitted regions | Terminators, off-by-one, over-wide loads |
| `machine.returned` | Did not return to the caller with the correct SP | The number of push/pop, the ret path, undefined instructions |
| `machine.abi.callee_saved` | A register that must be preserved changed | rbx, rbp, r12–r15 (x86), x19–x29 (aarch64) |
| `machine.abi.flags` | Returned with the x86 DF set | A `cld` after `std` |
| `effects.no_forbidden` | A forbidden effect such as a system call | `syscall` / `svc` |

The upper bits of narrow arguments (8/16/32 bits) are passed filled with garbage. A wrong `cmp` width causes a REJECT here.

## 11.3 Reading a HOLD

HOLD means "can be called neither pass nor fail". What to fix appears in `reasons`.

| reasons | Meaning | Action |
|---|---|---|
| `INCONCLUSIVE: ... BUDGET_EXHAUSTED` | The instruction-count limit was reached (it may not terminate) | The loop's exit condition. Or the Suite's `instructions_per_case` |
| `INCONCLUSIVE: ... UNSUPPORTED_DURING_RUN` / `ENGINE_ERROR` / `TIMEOUT` | An instruction the executor cannot handle (including undefined instructions), an executor failure, or a timeout | Review the instruction encoding and selection |
| `VACUOUS_SCOPE` | Zero cases satisfy `requires` | The Suite's generators (§11.4) |
| `NOT_EVALUATED: ... NATIVE_NOT_PERMITTED` | The Policy does not permit the native Executor | The trial zone (`policy.toml`). It does not switch to emulated automatically |
| `ENGINE_NOT_QUALIFIED` | This host's emulator has not passed engine qualification | See the record from `mukoz platform qualify --isa …` |
| `LOW_ADMITTED_CASES` | `requires` discarded most cases | The Suite's generators (§11.4) |

## 11.4 Writing contracts and Suites

- First check the syntax with `mukoz expr check '<expression>'` (types are checked when the contract is loaded). Make widths explicit for integers, as in `bv64(...)`.
- **Build dependent inputs with generators.** Do not discard them with `requires`. Make them depend on earlier variables through `len` (the length of bytes) and `max` (the upper bound of a bv) in `[generate.vars.<name>]`. Example: `len = "len(input.src) + len(input.src)"`. Build structured values (such as a sequence of fixed-length records) from simple variables with `expr` and `join` (`expr = '''join i in bv64(0)..input.n: <bytes expression of the record>'''`).
- Add values you want as boundaries to `values`.
- **See what was generated in `data.assessment.scope.input_summary`** (per-variable range, number of distinct values, count of zeros and empties). `plan_stats` shows how boundary values were generated (`boundary_mode`) and the number of cases discarded by `requires`.
- By default the start position of a region shifts per case (`placement = "varied"`). Code that assumes alignment fails here.
- Check that the contract you wrote gives ACCEPT on an implementation known to be correct and REJECT on one you broke on purpose.

## 11.5 Reading the scope

ACCEPT_WITHIN_SCOPE means "on the enumerated cases, on the emulator, the written properties were not violated". It is not a proof of correctness. `limitations` shows the limits of the scope (for example `enumerated_cases_not_exhaustive`, `emulated_only_not_native_execution`, `requires_excluded_*`, `boundary_product_reduced_*`). `release_authorized` is always false.

## 11.6 Building a process (a command-line program)

Details are in chapter 12. The key points:

- The target is `<isa>/raw/<abi>/linux` (starting with `_start`) or `<isa>/elf/<abi>/linux` (static ET_EXEC). At the entry, sp points at argc (same as Linux). Do not `ret` from the entry. End with exit / exit_group.
- The only usable system calls are read, write, open/openat, close, lseek, and exit/exit_group. Calling any other gives HOLD (`UNSUPPORTED_DURING_RUN`). Calling one not in the contract's `[effects] allow` gives REJECT.
- Only the paths declared in the Binding's `[files]` can exist. A raw process can use `data_bytes` (default 64 KiB) from `0x10000000` as a writable work area.
- The counterexample's `observed` shows stdout / stderr / exit code / file contents / recent system calls (arguments and return values). Look here and at `why_false` first.

## 11.7 Building in modules

- In `link.toml`, write the modules (file, exported symbols and offsets) and the import table (slot → symbol). Calls to other modules go through the table: on x86-64, `call qword ptr [0xf0000 + 8*k]`; on AArch64, `movz x16, #0xf, lsl #16; ldr x16, [x16, #8*k]; blr x16`.
- Build from the leaf modules and get each to ACCEPT with its own Suite before moving to the upper level. When checking an upper module, write the lower modules' Suites in `[monitors]`.
- If an upper-level REJECT shows `link.<symbol>.ensures` / `.abi` violated, fix the **callee**; if `.requires` is violated, fix the **caller**. `detail.blame` says which.
- Locations appear as `module-name+0x…`. With `--module <name>=<file>` you can swap in any module to try it.
