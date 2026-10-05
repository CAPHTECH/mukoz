# Mukoz

Mukoz checks whether an executable artifact satisfies a written contract. The artifact is a
binary, or a machine-code routine with an explicit entry point. Mukoz reports **under which
conditions and in which scope** the contract held, with counterexamples and machine-readable
evidence.

It is built for a loop in which an AI agent (or a person) writes or fixes a binary without
source code:

1. Write a contract.
2. Run `mukoz check`.
3. Read the counterexample, fix the binary, and check again.

Mukoz needs no source, debug information or compiler: it runs the machine code itself.

```text
contract.toml  what must hold: inputs, results, requires / ensures, allowed effects, termination
binding.toml   how the contract maps onto the binary: entry, registers, memory regions, files
suite.toml     which cases to run: boundary values, random cases, limits, executors
        │
        ▼
mukoz check suite.toml  ──►  ACCEPT_WITHIN_SCOPE / HOLD / REJECT
                             + a claim per property, counterexamples, scope, limitations (JSON)
```

Mukoz runs **enumerated cases, not proofs**. `ACCEPT_WITHIN_SCOPE` means that every planned case
satisfied every property within the reported scope. It says nothing about inputs outside that
scope. Anything Mukoz could not run or observe is `HOLD`, never success.

- [What to use it for](#what-to-use-it-for)
- [Status](#status-v010)
- [How it works](#how-it-works)
- [Quick start](#quick-start)
- [Writing a check](#writing-a-check)
- [Generating binaries with an AI agent](#generating-binaries-with-an-ai-agent)
- [Executors](#executors)
- [Commands](#commands)
- [Output, evidence and regression cases](#output-evidence-and-regression-cases)
- [What a verdict does and does not mean](#what-a-verdict-does-and-does-not-mean)
- [Build](#build)
- [Testing](#testing)
- [Repository layout](#repository-layout)
- [Documentation](#documentation)
- [Contributing, security, license](#contributing-security-license)

## What to use it for

Mukoz checks the finished binary, whatever produced it: a compiler, an assembler, a person, or
an AI agent writing machine code directly. A verdict rests on the contract and on what Mukoz
observed, not on how the binary was made ([docs/01](docs/01-concept.md)). It fits these uses:

1. **Checking the binaries that AI agents build.** An agent writes C, assembly or another
   compiled language, the toolchain builds it, and Mukoz checks the result that will ship. It
   checks after compiling and linking, and it checks the following on every case without being
   asked:
   - memory accesses outside the permitted regions, down to one byte (on the real CPU, memory
     protection works only per page);
   - ABI violations: a callee-saved register that changed, a direction flag left set, code that
     relies on the upper bits of narrow arguments;
   - code that assumes alignment, because the start of each region shifts from case to case.

   Functions in ELF relocatable objects (`.o`) can be checked directly, as long as they need no
   relocation. In the generation-loop trial, an agent took a C program with six seeded defects
   from `REJECT` to `ACCEPT_WITHIN_SCOPE`. It was told to work only from its directory and from
   Mukoz's output ([selfcheck/genloop](selfcheck/genloop/2026-10-05/README.md)).
2. **Changing binaries without source.** Patch a legacy or third-party binary, then check that it
   still meets its contract and that the old counterexample is fixed. For a program described
   as modules in a link file, `--module <name>=<file>` swaps in one module to try it in place.
   In the trials, an agent fixing gcc `-O3` output narrowed a one-byte error down from the
   violated property and the counterexample's inputs and outputs, then found the byte.
3. **Checking code for an ISA the host cannot run.** An x86-64 host checks AArch64 routines and
   processes in the emulator. The contract does not depend on the ISA, so a port can be checked
   against the contract of the original. This was the clearest benefit in the trials: without
   Mukoz, agents tested AArch64 code with simulators they wrote themselves, which could share
   their own misreading of the encoding.
4. **Grading low-level code in evaluations.** Write the contract once and judge any submission
   with it; the generator is never the judge. In the trials, Mukoz agreed with an independent
   hidden oracle on every judgement. A verdict is only as strong as the suite, though: one
   to-do mutation was exposed by 1 of 1570 generated cases.

The trials are summarized in [eval/RESULTS.md](eval/RESULTS.md).

It does not fit:

- proofs of correctness: Mukoz runs enumerated cases;
- finding a deliberately hidden backdoor, such as code that misbehaves for one 64-bit value:
  boundary and random cases are unlikely to hit it;
- floating-point code: floating-point arguments are unsupported, and the contract language has
  no floating-point values yet;
- timing side channels, such as checking that code runs in constant time;
- whole operating systems, GUI applications, dynamic linkers, language runtimes, or running
  malware ([docs/01 §1.5](docs/01-concept.md)).

## Status: v0.1.0

| Host | Status |
|---|---|
| Linux x86-64 | Tier 1: the acceptance criteria in [docs/09 §9.8](docs/09-implementation-plan.md) pass (`tools/tier1.py`) |
| macOS, Linux AArch64, Windows | Not built or tested |

What works on a Linux x86-64 host:

| Area | Implemented |
|---|---|
| Targets | x86-64 and AArch64 machine-code routines (SysV, AAPCS64, Apple arm64 ABI); static Linux ELF processes (x86-64, AArch64); Mach-O arm64 executables (`darwin-stdio/1`); functions of ELF relocatable objects; programs split into modules (link files with boundary monitors) |
| Executors | `emulated` (a separate emulator program, qualified on each host by known-answer tests); `native-routine` and `native-process` (the real CPU and kernel, inside a sandbox, only where an owner policy allows); differential tests between emulated and native execution |
| Commands | `check`, `show`, `replay`, `shrink`, `inspect`, `regressions`, `platform`, `expr check` |

Not implemented:

- macOS, Windows and Linux AArch64 hosts;
- dynamic linking, PIE and PE files;
- JSON-RPC.

The design documents describe more than is implemented. [docs/04 §4.10](docs/04-contract-language.md)
lists the implemented contract language exactly, and [docs/12](docs/12-process-and-modules.md)
covers processes and modules.

## How it works

1. **Load.** Mukoz reads the suite, the contract and the binding, and checks their types. It
   also inspects the artifact: format, entry point and code ranges.
2. **Plan.** It generates cases: a Cartesian product of boundary values, random cases from the
   suite's seed, and every stored regression case for this contract and target. Generation is
   deterministic, so the same files give the same cases.
3. **Execute.** It runs each case on each executor named in the suite. In the emulator, it
   watches every instruction and every memory access:
   - Execution must stay inside the code ranges.
   - Data accesses must fall inside the regions the binding allows, with the right access
     rights.
   - System calls go to an effect model, which permits only the effects the contract allows.
   - Callee-saved registers and the stack pointer must be restored.
4. **Judge.** It evaluates each `requires` / `ensures` expression on the observed state. It also
   checks the machine-level claims: return, ABI, memory access and effects.
5. **Assess.** It turns the claims into one admission, with the scope it covers and the
   limitations that apply.

| Admission | Meaning |
|---|---|
| `ACCEPT_WITHIN_SCOPE` | Every required claim was satisfied on every planned case, within the reported scope |
| `REJECT` | At least one property was violated. Each violation comes with a counterexample: inputs, observed values, the subexpressions that made it false, and the last instructions executed |
| `HOLD` | Mukoz could not decide. Examples: the instruction budget ran out, an instruction or system call is not supported, the engine is not qualified on this host, native execution is not permitted, too few cases were admitted. The reasons are listed |

## Quick start

Build (see [Build](#build) for requirements):

```sh
cargo build --release      # target/release/mukoz and target/release/mukoz-emu-icicle
```

Check a correct 64-bit add routine (`48 8d 04 37 c3`: `lea rax,[rdi+rsi]; ret`):

```sh
./target/release/mukoz check examples/add64/suite.x86_64.toml
#   "admission": "ACCEPT_WITHIN_SCOPE", 4160 cases
```

Check a mutant that subtracts instead:

```sh
./target/release/mukoz check examples/add64/suite.x86_64.toml --artifact fixtures/x86_64/add64_mut_sub.bin
#   "admission": "REJECT", "reasons": ["VIOLATED: arith.add64/sum"]
```

The first finding, abridged:

```json
{
  "property": "arith.add64/sum",
  "reason": "ENSURES_FALSE",
  "counterexample_id": "cx-…",
  "inputs": { "input.a": "0x0000000000000000", "input.b": "0x0000000000000001" },
  "observed": { "register.rax": "0xffffffffffffffff", "result.value": "0xffffffffffffffff" },
  "detail": {
    "expr": "result.value == input.a + input.b",
    "why_false": [["(result.value == (input.a + input.b))", "false"],
                  ["result.value", "bv64(0xffffffffffffffff)"],
                  ["(input.a + input.b)", "bv64(0x0000000000000001)"]]
  },
  "recent_instructions": [{ "offset": "0x0", "bytes": "4889f8" },
                          { "offset": "0x3", "bytes": "4829f0" },
                          { "offset": "0x6", "bytes": "c3" }],
  "stop": { "kind": "returned" }
}
```

Work with the counterexample:

```sh
./target/release/mukoz show <counterexample-id>                 # everything about it, paged
./target/release/mukoz show <counterexample-id> --disasm        # with disassembly
./target/release/mukoz shrink <counterexample-id>               # smaller inputs, same property and reason
./target/release/mukoz replay <counterexample-id> --artifact fixtures/x86_64/add64.bin   # rerun on the fixed binary
```

Evidence is stored under `.mukoz/` in the current directory (`--store <dir>` changes it). The
counterexample also becomes a **regression case**, which every later check of the same contract
and target runs first.

For CI, `--gate` turns the admission into the exit code: 0 for `ACCEPT_WITHIN_SCOPE`, 10 for
`HOLD`, 11 for `REJECT`. Without `--gate`, the exit code only says whether the operation itself
completed ([docs/07 §7.4](docs/07-interface.md)).

## Writing a check

A check is three TOML files. Here are the ones from `examples/add64/`.

**The contract** says what must hold, independent of any binary or ISA:

```toml
schema = "mukoz.contract/1"
id = "arith.add64"
boundary = "routine"            # or "process" for a whole program

[inputs]
a = "bv64"
b = "bv64"

[results]
value = "bv64"

[[ensures]]
id = "sum"
expr = "result.value == input.a + input.b"

[effects]
allow = []                      # no system calls

[termination]
kind = "must_return"
```

**The binding** maps it onto one binary and calling convention:

```toml
schema = "mukoz.binding/1"
id = "arith.add64@x86_64-sysv"
contract = "arith.add64"
target = "x86_64/raw/sysv-x86_64/none"     # isa / format / abi / os

[entry]
kind = "raw_offset"
offset = 0

[arguments]
rdi = "input.a"
rsi = "input.b"

[results]
value = "rax"

[completion]
kind = "return_to_sentinel"
```

**The suite** chooses the artifact and the cases:

```toml
schema = "mukoz.suite/1"
id = "arith.add64.x86_64"
contract = "contract.toml"
binding = "binding.x86_64.toml"

[artifact]
path = "../../fixtures/x86_64/add64.bin"

[generate]
seed = "20261004"
boundary = "product"      # Cartesian product of boundary values
random_cases = 4096

[limits]
instructions_per_case = 10000
wall_ms_per_case = 1000
```

The same contract has bindings for AArch64 (AAPCS64 and the Apple arm64 ABI) in the same
directory.

**Memory.** A routine that reads and writes memory declares regions. Mukoz places each region,
fills it, passes its address, and stops on any access outside the declared regions or rights.
From `examples/copy/`:

```toml
# contract: the copy writes only dst, and afterwards dst equals src
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

# binding: where the regions go and how the routine sees them
[arguments]
rdi = "addr(dst)"
rsi = "addr(src)"
rdx = "len(input.src)"
[regions.dst]
size = "len(before.dst)"
init = "before.dst"
access = "rw"
observe_as = "after.dst"
[regions.src]
size = "len(input.src)"
init = "input.src"
access = "r"
```

**Processes.** A contract with `boundary = "process"` describes a whole program:

- inputs: argv, stdin and files;
- results: stdout, stderr, files and exit status;
- the system calls it may make.

From `examples/hello/`:

```toml
boundary = "process"
[results]
code = "bv8"
out = { type = "bytes", max_len = 64 }
[[ensures]]
id = "greets"
expr = 'result.out == b"hello\n"'
[[ensures]]
id = "exit_zero"
expr = "result.code == bv8(0)"
[effects]
allow = ["write"]
[termination]
kind = "must_exit"
```

Expressions are typed and use bit-vectors (`bvN`), booleans and byte strings. Bounded
`forall` / `count` / `join`, slices, and little-endian reads are available. Integer literals give
their width (`bv8(0)`). `mukoz expr check '<expression>'` parses one and prints its normalized
form.

| Example | Shows |
|---|---|
| `add64`, `add32`, `zext32`, `cmp` | Register routines on x86-64 and AArch64; upper bits of narrow arguments; signed and unsigned comparison |
| `copy`, `strlen`, `checked_inc` | Memory regions with access rights; `requires`; state that must not change |
| `hello`, `todo` | Processes: stdout, argv, files and exit status, as raw images, static ELF and Mach-O; native differential runs |
| `link_sum3`, `link_copy2` | Programs from several modules, with boundary monitors that assign blame to the caller or the callee |

## Generating binaries with an AI agent

Mukoz was built for agents that write machine code directly. We ran a few small trials with
Claude models (October 2026). In these trials the agents had no assembler: they encoded
instructions by hand and laid the bytes out with Python. A hidden oracle, independent of Mukoz,
judged every submission after it was handed in.

- Sonnet 5.5 wrote correct programs without running them, up to the largest task we tried: a
  to-do command-line program of about 3–3.6 KB (3 of 3). Haiku 4.5 did not finish programs of
  that size: all 18 attempts stopped at stubs of under 610 bytes. Splitting the program into
  modules did not change this.
- Mukoz's verdicts agreed with the hidden oracle every time: 63 judgements in the larger trials
  and 36 final submissions in the first one. Mukoz accepted no wrong submission.
- In these trials Mukoz did not measurably raise the success rate. The tasks were either easy
  enough to be written correctly in one go, or too large to be finished at all. Its clearest
  benefit was independent checking for an ISA the host cannot run. Without Mukoz, agents wrote
  their own simulators, which could share their own misreading of the encoding.

The samples are small (1 to 4 runs per condition). [eval/RESULTS.md](eval/RESULTS.md) has the
details. It also compares the token cost of hand encoding with writing C or assembly and
letting a compiler or an assembler produce the bytes. [docs/11 §11.8](docs/11-agent-guide.md)
turns the trials into practical advice. The guide covers choosing a model for the size of the
task, encoding without an assembler, checking early, and making the suite strong enough. [docs/11](docs/11-agent-guide.md) is also the general guide
for agents: the generate-and-fix loop, how to read `REJECT` and `HOLD`, and how to write
contracts and suites.

## Executors

A suite lists its executors (`executors = ["emulated"]` by default). Mukoz never falls back from
one executor to another: if an executor cannot run, the result is `HOLD`.

### emulated

The subject runs in an emulator, in a separate program that `mukoz` starts as a child process
and drives over the line protocol `mukoz-emu/1` ([emu/PROTOCOL.md](emu/PROTOCOL.md)). The emulator
knows nothing about contracts. `mukoz` builds the memory and registers of each case, and answers
the emulator's system call, interrupt and breakpoint events. If the emulator crashes, that case
is `ENGINE_ERROR`, so the result is `HOLD`.

| Program | Engine | License | Build |
|---|---|---|---|
| `mukoz-emu-icicle` ([emu-icicle/](emu-icicle/)), the default | [icicle-emu](https://github.com/icicle-emu/icicle-emu): a SLEIGH / p-code interpreter that shares no code with Unicorn or QEMU | MIT OR Apache-2.0 | `cargo build --release` |
| `mukoz-emu` ([emu/](emu/)), optional | [Unicorn](https://github.com/unicorn-engine/unicorn) 2.1.1 (QEMU-derived) | GPL-2.0-or-later | `cargo build --release -p mukoz-emu` (needs CMake and a C compiler) |

`mukoz` starts `$MUKOZ_EMU` if it is set, else `mukoz-emu-icicle` next to its own executable.
[emu-icicle/README.md](emu-icicle/README.md) lists the known differences between the two
programs.

Before it trusts an engine on a host, Mukoz runs an **engine qualification**: known-answer
instruction tests whose expected values come from an independent reference, not from the engine.
On x86-64 hosts the same vectors also run on the real CPU. The record is stored per host, engine
build, ISA and test set. A missing or failed qualification makes every emulated result `HOLD`
(`ENGINE_NOT_QUALIFIED`). `mukoz platform qualify --isa <isa>` runs it explicitly.

### native-routine and native-process

These run the subject on the host CPU and kernel. They run only when an owner's `policy.toml`
(`--policy`) allows them, in one of two ways:

- The artifact is inside a **trial zone**: a directory declared for given executors and
  targets.
- The artifact's digest is listed explicitly.

The host probe must also confirm every isolation capability that the matching policy entry
requires. The capabilities are user, network and PID namespaces, chroot, resource limits, and
seccomp for routines. A digest entry may require none, so write the policy deliberately.

Without permission the result is `HOLD` (`NATIVE_NOT_PERMITTED`). Read
[docs/08](docs/08-security.md) before enabling them. `selfcheck/stage2/policy.toml` is a working
example.

With `executors = ["emulated", "native-routine"]` (or `"native-process"`), both run every case. Any
disagreement is a separate claim, `differential.emulated_vs_<native>`.

## Commands

Every command prints one JSON envelope on stdout:
`{"api_version": "mukoz/1", "command", "ok", "data", "errors"}`. `ok` means that the operation
completed, not that the subject passed.

| Command | Purpose |
|---|---|
| `mukoz check <suite.toml> [--artifact <file>] [--module <name>=<file>]… [--fail-fast] [--gate] [--store <dir>] [--policy <policy.toml>]` | Load, plan, run, judge and assess, in one step |
| `mukoz show <id> [--page <n>] [--disasm]` | A run, finding or counterexample, down to its instruction trace, in pages |
| `mukoz shrink <counterexample-id> [--budget <executions>]` | Shrink a counterexample while keeping the property, the reason and every `requires` |
| `mukoz replay <counterexample-id> [--artifact <file>]` | Rerun one counterexample, for example on a fixed binary |
| `mukoz inspect <file>` | Format check, entry point and load plan of an artifact |
| `mukoz regressions list <suite.toml>` / `prune <suite.toml> --case <id>` | List, or explicitly remove, the stored regression cases |
| `mukoz platform probe` / `show` / `qualify --isa <x86_64\|aarch64>` | Host probe, stored platform records, engine qualification |
| `mukoz expr check <expression>` | Parse an expression and print its normalized form |

## Output, evidence and regression cases

- Each `check` stores the following under `.mukoz/`:
  - the run, with its claims and findings;
  - each counterexample, with its case;
  - a copy of the artifact.
- The contract, binding and suite are recorded by path and digest, not copied. `replay` reads
  them again from their paths.
- `show` reads a run or a counterexample back by id.
- A verdict is tied to the exact artifact, contract, binding, suite, engine and host. A
  `replay` on a changed artifact is marked as such. Mukoz never reuses an old verdict for
  changed inputs.
- Every counterexample becomes a regression case for its contract and target. It keeps the same
  id and the same execution, including the filler seed, across runs. Later checks run these
  cases first.
- Every assessment records **checker independence**. This tells whether the subject is Mukoz
  itself, or was produced by a previous version of it ([docs/06 §6.9](docs/06-evidence-and-judgement.md)).

## What a verdict does and does not mean

- `ACCEPT_WITHIN_SCOPE` covers only the enumerated cases, on the executors that ran, on this
  host. `scope` lists them, with a summary of the generated inputs. `limitations` lists what the
  check did not cover, for example `enumerated_cases_not_exhaustive` or
  `emulated_only_not_native_execution`.
- `release_authorized` is always `false`. Mukoz provides evidence; the decision to ship
  belongs to a person.
- Several situations are never success:
  - an empty scope;
  - running out of the budget;
  - an unsupported instruction or system call;
  - a timeout;
  - a missing claim;
  - too many cases excluded by `requires`.

  Each of these is `HOLD`.
- Non-termination cannot be shown by a finite run. A routine that runs out of instructions is
  therefore `HOLD` (`BUDGET_EXHAUSTED`), not `REJECT`.

[docs/01](docs/01-concept.md) states the boundary of guarantees in full.

## Build

Requirements:

- Rust 1.97.1, pinned in `rust-toolchain.toml`.
- A C compiler. Capstone, used for `show --disasm`, is built from source. The optional Unicorn
  emulator also needs CMake.
- Network access for the first build. icicle-emu is a git dependency pinned to one commit.

```sh
cargo build --release                   # mukoz and mukoz-emu-icicle
cargo build --release -p mukoz-emu      # optional: the Unicorn emulator (GPL)
```

`--no-default-features` builds `mukoz` without Capstone. Only `show --disasm` changes.

Without an emulator program, emulated results are `HOLD`
(`ENGINE_NOT_QUALIFIED: EMULATOR_UNAVAILABLE`). The native executors still work.

## Testing

```sh
cargo test --release          # unit, acceptance, negative, platform, self-check and protocol tests
python3 tools/tier1.py        # the Tier 1 acceptance criteria, written to target/tier1-report.json
python3 selfcheck/run.py      # self-check stages 1–3 (docs/09 §9.6)

# the same tests with the Unicorn emulator
cargo build --release -p mukoz-emu
MUKOZ_EMU=$PWD/target/release/mukoz-emu cargo test --release --workspace --exclude mukoz-kernels
```

Some tests run native executors. They need a Linux x86-64 host with unprivileged user
namespaces enabled (on Ubuntu 24.04: `sysctl kernel.apparmor_restrict_unprivileged_userns=0`).
[.github/workflows/ci.yml](.github/workflows/ci.yml) runs all of the above on every push to
`main`.

## Repository layout

| Path | Contents |
|---|---|
| `src/` | The `mukoz` CLI: `spec` (file formats), `expr` (expressions), `plan` (case generation), `emu` (emulated executor and effect model), `native` / `nproc` (native executors), `host` (probe and isolation), `policy`, `qualify` (engine qualification), `judge` (claims and admission), `run` (`check`, `replay`), `show`, `store`, `image` (loaders) |
| `emu-icicle/` | `mukoz-emu-icicle`, the default emulator program, with the Ghidra processor specifications it embeds (`sleigh/`) |
| `emu/` | `mukoz-emu`, the optional Unicorn emulator program, and the protocol specification (`PROTOCOL.md`) |
| `examples/` | Contracts, bindings and suites |
| `fixtures/` | Reference binaries and mutants for every ISA and format, with their sources (`build.sh`) |
| `tests/` | Integration tests |
| `selfcheck/` | Mukoz checking itself: its kernels as routines on both ISAs, the static CLI as a process, and a generation-loop record |
| `tools/` | `tier1.py` (acceptance criteria) and the generator of the qualification vectors |
| `eval/` | The comparison trials (agents with and without Mukoz): tasks, hidden oracles, results; see [eval/README.md](eval/README.md) |
| `docs/` | Specification and design |

## Documentation

[docs/README.md](docs/README.md) is the index:

| Chapter | Topic |
|---|---|
| 01 | Concept and the boundary of guarantees |
| 02 | Platform model |
| 03 | Architecture and data model |
| 04 | Contract, binding and suite |
| 05 | Artifact inspection and execution |
| 06 | Evidence and judgement |
| 07 | Interface (CLI) |
| 08 | Security |
| 09 | Implementation plan, testing and self-check |
| 10 | Design decisions |
| 11 | Usage guide for agents, including advice for generating machine code |
| 12 | Processes and modules |

Each chapter says what is implemented and what is design only.

## Contributing, security, license

- **Contributing.** We do not accept pull requests at this time. Bug reports are welcome as
  issues ([CONTRIBUTING.md](CONTRIBUTING.md)).
- **Security.** Report vulnerabilities privately ([SECURITY.md](SECURITY.md)).
- **License.**
  - Mukoz (this repository, except `emu/`) is licensed under either the MIT license
    ([LICENSE-MIT](LICENSE-MIT)) or the Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at
    your option. Copyright (c) 2026 CAPH TECH Inc.
  - `mukoz-emu-icicle` embeds Ghidra processor specifications under the Apache License 2.0
    ([emu-icicle/sleigh/LICENSE](emu-icicle/sleigh/LICENSE),
    [emu-icicle/sleigh/NOTICE](emu-icicle/sleigh/NOTICE)).
  - `mukoz-emu` ([emu/](emu/)) links Unicorn and is licensed under the GPL, version 2 or later
    ([emu/LICENSE](emu/LICENSE)). It is built only on request. It is a separate program: `mukoz`
    talks to it over the documented protocol, and neither links the other.
