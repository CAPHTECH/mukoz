# 11. Usage for agents

The procedure an AI agent follows to generate or fix a binary without source and to check with Mukoz whether it satisfies its contract. The same checks apply to compiled binaries. Section 11.9 covers access policies and other security contracts. The implemented scope is in chapter 04 §4.10 and chapter 07 §7.2. If they disagree, the implementation is correct.

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
| `machine.memory.access` | Access outside a permitted region or contrary to its read/write permissions | Terminators, off-by-one, over-wide loads, reads from write-only buffers, temporary writes to read-only inputs |
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

## 11.8 Generating machine code: advice from the trials

These points come from the comparison trials in `eval/RESULTS.md`. In those trials, Claude
agents encoded instructions by hand, without an assembler, and a hidden oracle judged each
submission. Each point says what was observed; the counts are small (1–4 runs per condition).

- **Match the model to the size of the program.**
  - Observed: Sonnet 5.5 wrote correct programs without running them, up to a 3–3.6 KB to-do
    command-line program (3 of 3).
  - Observed: Haiku 4.5 did not finish programs of that size. All 18 attempts stopped at stubs
    under 610 bytes, and splitting into 8 modules did not help, because main was still not
    written. Haiku managed some small routines, such as memmove (8 of 12 across conditions), but
    none of utf8_to_utf16 (0 of 24).
  - A model that stops at a stub is not making mistakes that a counterexample can fix. Give it
    smaller pieces, or a stronger model. Whether finer splits rescue a smaller model is untested
    `[U]`.
- **Encode carefully; use an assembler when you may.**
  - Observed encoding mistakes include a wrong `jcc` opcode and a wrong `cmp` immediate.
  - Observed once: a Sonnet agent wrote a small instruction encoder in Python, and its todo2
    program (one of three, 3–3.6 KB) passed. A table of opcodes and one function per form keeps the bytes
    consistent `[R]`.
  - When the task allows an assembler, using one removes this class of error `[R]`. Mukoz
    checks the result either way.
- **Check early, with the real contract.**
  - Run `mukoz check --fail-fast` on a partial program, or on one routine at a time
    (§11.7), instead of writing everything first.
  - Observed: an agent narrowed a bug in an unrolled base64 loop from the property id (`char2`)
    and the counterexample's inputs and outputs to "only odd groups", and then found the byte.
    In a later run, `detail.why_false`, added after that trial, pointed to the same place.
- **For another ISA, let Mukoz be the independent check.**
  - Observed: without Mukoz, every agent writing AArch64 on an x86-64 host built its own
    simulator to test with. One simulator had its own bug, and all of them noted that their
    encoder and decoder could share a misreading.
  - Mukoz's emulator and its qualified engine do not share your understanding of the encoding.
- **Process programs:**
  - Read §11.6 before writing `_start`. Observed: one agent miscounted argc and fixed it from a
    counterexample.
  - Check stdout, the exit status and files in `observed` first.
- **A suite is only as strong as its cases.**
  - Observed: one to-do mutation was exposed by 1 of 1570 generated cases.
  - Add edge values with `[generate.vars.<name>] values`.
  - Build structured inputs with `expr` instead of filtering them with `requires`.
  - Confirm that the suite rejects mutations you make on purpose (§11.4).
- **When running agents in a harness:**
  - Give each agent its own working directory, and check that nothing was written outside it.
    Observed: one agent wrote into the repository.
  - Stop subagents when the agent reports. Observed: one spawned subagent kept editing after
    the report.

## 11.9 Checking security contracts

Use this procedure when a returned value can be correct while an intermediate operation
violates the caller's policy. The applications and recorded results are in
[01 §1.8](01-concept.md#18-applying-mukoz-to-security-contracts).

1. Define the unit and its policy before checking the candidate. Identify the entry point,
   inputs, initial memory, permitted reads and writes, allowed effects and completion condition.
2. Write the expected result in the Contract and map it to the binary in the Binding. For
   memory policy, set region sizes and `access` separately from final-state observations.
   `access = "r"` forbids even a write that is undone before return; `access = "w"` forbids
   reading existing contents. A frame condition only checks what remains unchanged at exit.
3. Use an emulated Suite with the required claims enabled. Include boundary inputs and a
   permitted control as well as deliberate violations. For modules, configure the callee
   monitors (§11.7); for effects, choose a supported model (§11.6).
4. Run `check`, inspect the violated property and finding, and replay its counterexample
   with the same store. After fixing the artifact, replay against the replacement and run
   the full Suite without `--fail-fast`.
5. Read the admission together with scope and limitations (§11.5). Keep HOLD and unobserved
   properties visible. Do not drop a required claim or widen a permission to make a candidate
   pass unless the intended policy actually changes.

From the repository root, the supplied fixtures can be checked without rebuilding their
assembly or running them natively:

```sh
cargo build --release

# Correct copy: ACCEPT_WITHIN_SCOPE.
./target/release/mukoz check examples/security_access/suite.x86_64.toml --store .mukoz

# Same expected final data, but a temporary write to read-only src: REJECT.
./target/release/mukoz check examples/security_access/suite.x86_64.toml \
  --artifact examples/security_access/x86_64/restore_source.bin --store .mukoz

# Read from a write-only audit buffer on AArch64: REJECT.
./target/release/mukoz check examples/security_access/suite.aarch64.toml \
  --artifact examples/security_access/aarch64/read_audit.bin --store .mukoz

# Substitute an ID from the check's claim.counterexamples or finding.
./target/release/mukoz replay <counterexample-id> --store .mukoz
```

For `machine.memory.access`, inspect the instruction offset, address, access width,
read/write direction and the region permission. Execution stops at the forbidden access;
unfinished output checks remain INCONCLUSIVE. Widening `src` from `r` to `rw` permits the
temporary write by the same binary: the verdict depends on the supplied policy.

The other examples exercise distinct obligations:

| Question | Runnable example | Finding to inspect |
|---|---|---|
| Does the routine preserve its caller's registers? | `security_checks/sum.x86_64.suite.toml` with `fixtures/x86_64/add64_mut_clobber_rbx.bin` | `machine.abi.callee_saved`; the sum can still be satisfied |
| Does the caller obey its callee's precondition? | `security_checks/module.strict.x86_64.suite.toml` | `link.arith.add64.requires`, `blame=caller`; the final sum can still be satisfied |
| Is a narrow signed argument extended correctly? | `security_checks/signed.x86_64.suite.toml` with `security_checks/x86_64/zero_extend.bin` | `arith.sext32/sign_extended` |
| Is output permitted by this process contract? | `security_checks/output.allowed.aarch64.suite.toml` and `security_checks/output.denied.aarch64.suite.toml` | The same binary passes the first and violates `effects.no_forbidden` in the second |

Paths beginning with `security_checks/` in this table are relative to `examples/`.
Use `--artifact` for the replacement binaries. The
[access example guide](../examples/security_access/README.md) and
[further checks guide](../examples/security_checks/README.md) provide the complete commands,
both ISA variants, permitted controls and matrix runner with counterexample replay.

These examples require no native execution or VM. They check explicit policies within
the emulator's supported model and enumerated cases. They do not infer authorization,
prove complete ABI compliance or control-flow integrity, or establish a real application's
network policy. Module monitors also do not enforce separate per-callee memory permissions
([12](12-process-and-modules.md)).
