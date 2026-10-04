# 04 Contracts, Binding, Suite

## 4.1 Write the three separately

```text
Contract: returns the 64-bit wrapping sum of a and b.          (ISA-independent)
Binding:  on x86_64 SysV, a=rdi, b=rsi, result=rax.            (platform-dependent)
Suite:    Cartesian product of boundary values plus 4096 random cases with a fixed seed. (checking method)
```

- A Contract knows nothing about register assignment. Several Bindings, one per ISA, can be attached to the same Contract.
- A Binding does not define what is correct.
- A Suite does not remove required claims.

## 4.2 File format

- The format humans write is **TOML**. The canonical form between machines is **JSON**. Both are converted to the same internal model and identified by the digest of the normalized JSON.
- Unknown fields and duplicate keys are rejected.
- Expressions are written as **strings** inside TOML/JSON (4.3) and converted to a typed AST on load. The normalized JSON stores the AST, not the expression string.
- Each file carries a version, as in `schema = "mukoz.contract/1"`.

## 4.3 Expression language

Arbitrary Python, JavaScript, and shell are not allowed. Only a small typed expression language is interpreted.

### Types

| Type | Meaning |
|---|---|
| `bool` | Boolean |
| `bv8` `bv16` `bv32` `bv64` | Fixed-width bit vector. Arithmetic is modular at that width |
| `bytes` | Variable-length byte string. `max_len` is required at declaration |

Mathematical integers, floating point, and widths of 128 bits or more will be added in a later version.

### Syntax

```text
expr    := or_expr
or_expr := and_expr ("or" and_expr)*
and_expr:= not_expr ("and" not_expr)*
not_expr:= "not" not_expr | cmp
cmp     := bitor (("==" | "!=") bitor)?
bitor   := bitxor ("|" bitxor)*
bitxor  := bitand ("^" bitand)*
bitand  := add ("&" add)*
add     := mul (("+" | "-") mul)*
mul     := unary ("*" unary)*
unary   := "~" unary | postfix
postfix := primary ("[" expr "]")*
primary := path | literal | call | "(" expr ")"
        | "forall" IDENT "in" expr ".." expr ":" expr
path    := IDENT ("." IDENT)*
call    := IDENT "(" (expr ("," expr)*)? ")"
literal := "true" | "false"
        | "bv" WIDTH "(" (HEX | DEC) ")"      # bv64(0xff), bv8(10)
        | 'b"' ... '"'                         # bytes, with \n and \xNN escapes
        | 'hex"' HEXDIGITS '"'                 # bytes
```

- `+ - * & | ^ ~` apply only to bv operands of the same width. **There is no implicit width conversion.**
- Ordered comparisons must always name their signedness interpretation: `ult ule ugt uge slt sle sgt sge`. There is no `<` symbol.
- Other functions: `ite(c, a, b)`, `zext(x, 64)`, `sext(x, 64)`, `extract(x, hi, lo)`, `shl(x, n)`, `lshr(x, n)`, `ashr(x, n)`, `len(b)` (bv64), `slice(b, off, n)`, `concat(a, b)`.
- `b[i]` is the i-th element of bytes (bv8). An out-of-range index is an evaluation error and counts as neither success nor failure (`EVALUATION_ERROR` → the claim is INCONCLUSIVE).
- `forall i in lo..hi: p` is a bounded quantification over the half-open interval `[lo, hi)`. `i` is bv64. The length of the range must not exceed the Suite limit (default 65,536).
- `==` works on bv, bool, and bytes. Comparing bytes requires equal length and all bytes equal.

### Variable namespaces

| Namespace | Meaning |
|---|---|
| `input.*` | Inputs. Immutable values at the start of execution |
| `before.*` | State values at the start |
| `after.*` | State values at the end |
| `result.*` | Return values of the routine |
| `stdout` `stderr` `exit.*` | Observations at the process boundary (4.5) |

Evaluating an expression does not re-run the subject and does not read host files or environment variables. The limits are depth 64 and 8,192 nodes.

## 4.4 Contracts at the routine boundary

### Example 1: 64-bit wrapping addition

```toml
schema = "mukoz.contract/1"
id = "arith.add64"
boundary = "routine"

[inputs]
a = "bv64"
b = "bv64"

[results]
value = "bv64"

[[ensures]]
id = "sum"
expr = "result.value == input.a + input.b"

[effects]
allow = []

[termination]
kind = "must_return"
```

The full property id is `arith.add64/sum`. Omitting `requires` means "always true".

### Example 2: buffer copy (variable length)

```toml
schema = "mukoz.contract/1"
id = "mem.copy"
boundary = "routine"
modifies = ["dst"]

[inputs]
src = { type = "bytes", max_len = 256 }

[state]
dst = { type = "bytes", max_len = 256 }

[[requires]]
id = "same_len"
expr = "len(before.dst) == len(input.src)"

[[ensures]]
id = "copied"
expr = "after.dst == input.src"

[effects]
allow = []

[termination]
kind = "must_return"
```

`modifies` is a top-level key, so in TOML it must come before any table heading. For state not listed in `modifies`, a claim that the state at the end equals the state at the start is generated automatically (`frame`).

### Example 3: stateful counter (failure path stated explicitly)

```toml
schema = "mukoz.contract/1"
id = "counter.checked_inc"
boundary = "routine"
modifies = ["count"]

[inputs]
n = "bv64"

[state]
count = "bv64"

[results]
status = "bv32"

[[ensures]]
id = "ok_path"
expr = "(result.status == bv32(0)) == (not ult(before.count + input.n, before.count))"

[[ensures]]
id = "ok_updates"
expr = "ite(result.status == bv32(0), after.count == before.count + input.n, after.count == before.count)"

[effects]
allow = []

[termination]
kind = "must_return"
```

The contract states that on overflow the routine "leaves the state unchanged and returns nonzero". If it did not say so, that behavior would not be checked (chapter 01 §1.4).

## 4.5 Contracts at the process boundary

Version 0.4 had no way to write a program's stdout or exit code in a contract. `boundary = "process"` is added.

| Observation | Type |
|---|---|
| `stdout` `stderr` | bytes (`max_len` is the Suite output limit) |
| `exit.exited` | bool (whether it exited normally) |
| `exit.code` | bv32 (the OS exit code. On Linux only the low 8 bits are meaningful) |
| `exit.signaled` / `exit.signal` | bool / bv32 (when it ended by a signal) |
| `input.stdin` | bytes |

```toml
schema = "mukoz.contract/1"
id = "hello.smoke"
boundary = "process"

[inputs]
stdin = { type = "bytes", max_len = 0 }

[[ensures]]
id = "stdout"
expr = 'stdout == b"Hello from raw x86-64 ELF!\n"'

[[ensures]]
id = "exit_zero"
expr = "exit.exited and exit.code == bv32(0)"

[[ensures]]
id = "no_stderr"
expr = "len(stderr) == bv64(0)"

[effects]
allow = ["write:1", "exit"]
```

### Effect vocabulary

`effects.allow` lists the effects that cross the boundary and are permitted. All others are forbidden.

| Effect | Meaning |
|---|---|
| `write:<fd>` | Write to the given fd |
| `read:<fd>` | Read from the given fd |
| `exit` | Process exit |
| `mem:<region>` | The routine has an effect outside the given region (Binding). Normally not written |

For a forbidden effect, the attempt itself is a violation (chapter 06 §6.3). On an Executor that cannot observe effects, this claim becomes NOT_EVALUATED.

### Write robustness contracts separately

The hello example is a smoke contract for an environment in which write always succeeds in full. Whether the program "writes everything or ends with the specified error" in an environment where short writes or failures occur is checked with a separate contract and a separate environment model. Do not claim robustness from the success of a smoke contract.

## 4.6 Binding

### Routine (x86_64 SysV)

```toml
schema = "mukoz.binding/1"
id = "arith.add64@x86_64-sysv"
contract = "arith.add64"
target = "x86_64/raw/sysv-x86_64/none"

[entry]
kind = "raw_offset"
offset = 0

[arguments]
rdi = "input.a"
rsi = "input.b"

[results]
value = "rax"

[stack]
bytes = 16384

[completion]
kind = "return_to_sentinel"
```

### AArch64 Binding for the same contract

```toml
schema = "mukoz.binding/1"
id = "arith.add64@aarch64-aapcs64"
contract = "arith.add64"
target = "aarch64/raw/aapcs64/none"

[entry]
kind = "raw_offset"
offset = 0

[arguments]
x0 = "input.a"
x1 = "input.b"

[results]
value = "x0"

[stack]
bytes = 16384

[completion]
kind = "return_to_sentinel"
```

### Code regions

- For a raw routine, omitting `code_regions` makes **the whole file one code region**. Even if the AI changes instruction lengths on each generation, the Binding does not need to be rewritten.
- When instructions and data share a file, state `[[code_regions]]` (`offset`, `size`) and `[[data_regions]]` explicitly. The sizes must not exceed the file length (`BINDING_MISMATCH`).
- A Binding **does not say which file to check**. The artifact is given by the Suite or the CLI (4.7). This lets one Binding be reused for artifacts that change on every generation.

### Routine that uses regions (mem.copy, x86_64 SysV)

```toml
schema = "mukoz.binding/1"
id = "mem.copy@x86_64-sysv"
contract = "mem.copy"
target = "x86_64/raw/sysv-x86_64/none"

[entry]
kind = "raw_offset"
offset = 0

[regions.dst]
size = "len(before.dst)"
init = "before.dst"
access = "rw"
observe_as = "after.dst"

[regions.src]
size = "len(input.src)"
init = "input.src"
access = "r"

[arguments]
rdi = "addr(dst)"
rsi = "addr(src)"
rdx = "len(input.src)"

[completion]
kind = "return_to_sentinel"
```

- The harness places each region at a separate location and puts an **unallocated guard region** between them. It also detects out-of-region accesses with byte-level access monitoring.
- The placement of regions (alignment and relative position) can be a generation target of the Suite (4.7).
- Pointer arguments are fixed at their entry values and are not re-resolved if a register is later overwritten. Reading state uses `observe_as` to point at a region.
- For each region, mapped, initialized, readable, writable, and unchanged at exit can be specified separately. The default is derived from `access`.

### Machine claims the Binding adds automatically

| claim | Content |
|---|---|
| `machine.returned` | Returned to the return address prepared at entry, with the correct SP (chapter 05 §5.3) |
| `machine.abi.callee_saved` | The ABI's callee-saved registers equal their entry values |
| `machine.abi.stack` | SP is restored, and alignment holds at the specified points |
| `machine.abi.reserved` | Reserved registers are not used (example: x18 on Apple / Windows ARM64) |
| `machine.memory.access` | Every access falls entirely within a permitted region |
| `effects.no_forbidden` | No attempt at a forbidden effect |

The content for each ABI is set by the table in chapter 05 §5.4.

### Process (x86_64 Linux ELF)

```toml
schema = "mukoz.binding/1"
id = "hello.smoke@x86_64-linux"
contract = "hello.smoke"
target = "x86_64/elf/sysv-x86_64/linux"

[entry]
kind = "format_entry"          # ELF e_entry

[process]
argv = ["hello"]
env = {}

[environment]
model = "linux-stdio/1"
write_results = "full-success/1"
```

## 4.7 Suite

```toml
schema = "mukoz.suite/1"
id = "arith.add64.primary"
contract = "arith.add64"
binding = "arith.add64@x86_64-sysv"
executors = ["emulated"]

[artifact]
path = "build/add64.bin"      # Path relative to the Suite file. Can be overridden by the CLI's --artifact

[generate]
seed = "20261004"
boundary = "product"          # Cartesian product of the default boundary values per type
random_cases = 4096

[limits]
instructions_per_case = 100000
wall_ms_per_case = 1000
guest_memory_bytes = 16777216
trace_bytes_per_case = 4194304
```

- `path` in `[artifact]` only shows the location. Identity is decided by the digest of the snapshot taken at load time. The Plan is bound to that digest, and the digest is checked immediately before and after execution (chapter 08 §8.5).
- If neither `[artifact]` nor `--artifact` is given, this is a usage error (exit code 2), not `CONTRACT_GAP`.
- The default boundary values for bv64 are eight: `0, 1, 2, 0x7fff…ffff, 0x8000…0000, 0xffff…fffe, 0xffff…ffff, 0x0101…0101`. The Cartesian product of two inputs gives 64 cases, and with the random cases 4,160. Duplicates are not removed, and the generation order is included in the case ID. The number of duplicate inputs is reported, and the result is never described as "4,160 distinct inputs".
- Boundary values can be added or controlled with `[generate.vars.<variable name>]` (implemented items are in 4.10).
- For bytes, the defaults are length 0, 1, `max_len`, and a random length.
- The required claims are, by default, "all ensures of the Contract + frame + the Binding's machine claims". A Suite can add claims but cannot remove them. Removal is done only by Policy, and that fact is recorded in the evidence.
- **Regression cases:** Past counterexamples are added automatically as regression cases (4.8). They can be excluded with `[regressions] include = false`, but the exclusion is recorded in the evidence and in the output's `limitations`.
- If `executors` lists several, the same cases run on several Executors and the differences are compared (chapter 06 §6.8).
- If a limit exceeds the Policy limit, the plan is rejected and the limit is not silently truncated.
- If there are 0 valid cases (those that satisfy the precondition), the result is HOLD (`VACUOUS_SCOPE`).
- If the valid cases are below the lower bound, the result is HOLD (`LOW_ADMITTED_CASES`). The default lower bound is min(100, 1/4 of the generated count). A precondition that discards most cases is a sign that the generator cannot produce the inputs the contract assumes, and a pass on the few remaining cases would make the scope look larger than it is. To narrow deliberately, state the lower bound explicitly with `[limits] min_admitted_cases`. The number of discarded cases is always shown in `limitations`.

## 4.8 Regression cases

When the AI repeats generation and checking, counterexamples are accumulated **bound to the contract and the target, not to the artifact**, so that each round can confirm that a previously found failure has not recurred.

```text
.mukoz/regressions/<contract digest>/<target platform>/<case digest>.json
```

- Stored content: the semantic inputs (`input.*`, `before.*`), the environment response script, the region placement, the initial register values the Binding did not specify, the original counterexample ID, and whether it has been shrunk.
- If a shrunk counterexample exists, store it; otherwise store the original counterexample. If both exist, store both.
- In the next `plan` / `check`, regression cases are placed **before** the generated cases, and their case IDs start with `reg-`. Their count is reported separately from the generated cases.
- A regression case remains usable when the Binding changes, as long as the contract and the target are the same. If the ABI is the same, the initial register values are used as they are. If a Binding change means a regression case cannot be applied (for example, a needed variable is missing), that case is reported as NOT_EVALUATED and not silently dropped.
- If the contract's digest changes, the old regression set is reported only by count, as **not applicable**. It is not migrated automatically to the new contract (the meaning of the expected values may change).
- Regression cases are never deleted automatically. If the limit (default 1,024) is exceeded, the plan is rejected (`REGRESSION_LIMIT_EXCEEDED`) and cases are not silently thinned. Cleanup is done explicitly with `mukoz regressions prune`, and the operation is recorded.
- A regression case passing means "that counterexample has not recurred". It does not mean overall correctness (chapter 06 §6.10).

## 4.9 Missing and contradictory contracts

| Situation | Report |
|---|---|
| Required information is missing (type, completion condition, etc.) | `CONTRACT_GAP` |
| Types do not match | `CONTRACT_TYPE_ERROR` |
| The generator cannot produce cases that satisfy the precondition | `VACUOUS_SCOPE` (not asserted to be a logical contradiction) |
| The variables of the Binding and the Contract do not correspond | `BINDING_MISMATCH` |

## 4.10 Implementation status (Mukoz 0.1.0; the process boundary and module splitting are in chapter 12)

**The implemented form of the process boundary (`boundary = "process"`), files, link files, and boundary monitoring is written in chapter 12.**

This chapter is the design, and the implementation is a subset of it. **When they differ, the implementation (the tool's error messages) is correct.** For agents that write contracts, the implemented scope is fixed here.

| Item | Implementation in 0.1.0 |
|---|---|
| Suite `contract` / `binding` | **File paths** (relative to the Suite). Referencing by ID is not implemented |
| `executors` | `["emulated"]` (default), `["native-routine"]`, `["native-process"]`, `["emulated", "native-routine"]`, `["emulated", "native-process"]` (differential test, claim `differential.emulated_vs_<native>`). Native without Policy permission is NOT_EVALUATED (`NATIVE_NOT_PERMITTED`) and gives HOLD. There is no automatic substitution between Executors |
| `[entry] kind` | `raw_offset`, `elf_entry` (ELF executable), `macho_entry` (Mach-O, chapter 12), `symbol` (link file), `object_symbol` (a function symbol in an ELF relocatable object ET_REL, as the routine. `symbol = "name"`. The target is `<isa>/elf/<abi>/none`. If the function contains a relocation, the result is `UNRESOLVED_DEPENDENCY`: Mukoz does not link) |
| `[selfcheck] checkers` | When the subject is Mukoz itself, a file listing the checkers of the previous version (chapter 06 §6.9). When given, `assessment.independence` becomes `self` / `previous_version`. When not given, it is `independent` |
| Regression cases | The ID is `reg-<stored digest>` and is the same across runs. The stored filler seed is used as is (the same execution as the original counterexample) |
| `[generate]` | `seed`, `boundary` (`product` / `none`), `random_cases` |
| `[generate.vars.<name>]` | `values`: additional values (bv as a decimal / 0x-hex string, bytes as a hex string; not the `hex"..."` form). `len`: expression for the length of bytes (may refer to earlier variables; example `len(input.src) + len(input.src)`). `max`: expression for the upper bound of bv (inclusive; may refer to earlier variables). `bytes`: value range of bytes, `nonzero` / `ascii`. `pieces`: build bytes by concatenating weighted hex fragments (`"c280:20"`). `expr`: give the value directly as an expression over earlier variables (derived input; example: build well-formed file contents from simple variables. Cannot be combined with other items; exceeding `max_len` gives `PLAN_ERROR`). An item that does not fit the type is an error |
| Boundary values | For bv, {0, 1, max/2, max−1, max} if `max` is given, otherwise the 4.7 default. For bytes, the single length given by `len` if present, otherwise {0, 1, max_len/2, max_len}. `values` is added to these. If the Cartesian product exceeds `max_cases/2`, it switches to "one variable at a time at boundary values, the others random", and reports `plan_stats.boundary_mode = "one_at_a_time"` and `limitations` |
| Check of generated inputs | `data.assessment.scope.input_summary` shows the range for each variable (bv: min, max, number of distinct values, count of 0 and all-ones; bytes: min and max length, number of distinct lengths, count of empty; bool: counts) |
| Non-termination | With `must_return`, reaching the instruction limit gives HOLD (`BUDGET_EXHAUSTED`). Non-termination cannot be shown by a finite execution, so it is not REJECT (I1) |
| Dependent generation | Variables referenced by `len` / `max` are generated first (a cycle is an error). If `len` exceeds the contract's `max_len`, the result is `PLAN_ERROR` (no truncation) |
| `[limits]` | `instructions_per_case`, `wall_ms_per_case`, `max_cases` (upper limit 8192), `min_admitted_cases`. `guest_memory_bytes` / `trace_bytes_per_case` are not implemented |
| Generation of region placement | `[generate] placement = "varied"` (default): shifts the start alignment of each region by 0 to 15 bytes per case (derived from the case's seed, so re-runs and regressions get the same placement). `"aligned"` fixes it to a 16-byte boundary. Relative placement between regions and placement at high addresses are not implemented |
| machine claims | `machine.returned` (including SP restoration), `machine.abi.callee_saved`, `machine.abi.flags` (x86 DF), `machine.abi.reserved` (x18 of apple-arm64 only), `machine.memory.access`, `effects.no_forbidden`. `machine.abi.stack` is not an independent claim and is included in `machine.returned` |
| Expressions | The typed expressions of 4.4. Integer literals state the width explicitly as `bvN(...)` (a bare integer in a contract is `CONTRACT_TYPE_ERROR`). `mukoz expr check` checks syntax only; types are checked when the contract is loaded |
| Range of `forall` / `count` / `join` | Up to 65,536 per expression. `join i in a..b: <bytes>` concatenates the bodies (the result is up to 1 MiB) |
| Additional functions | `dec(bv)`: unsigned decimal string (bytes). `u16le` / `u32le` / `u64le(bytes, off)`: little-endian reads (out of range is an evaluation error → INCONCLUSIVE) |
| Process Binding and environment model | Chapter 12 |
