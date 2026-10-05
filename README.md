# Mukoz

Mukoz checks whether an executable artifact — a binary, or a machine-code routine with an explicit
entry point — satisfies a written contract, and reports **under which conditions and in which
scope** it did, with counterexamples and machine-readable evidence.

It is built for a loop in which an AI agent (or a person) writes or fixes a binary without source
code: write a contract, run `mukoz check`, read the counterexample, fix, check again.

```text
contract.toml  (what must hold: inputs, results, requires / ensures, allowed effects)
binding.toml   (how the contract maps onto the binary: entry, registers, memory regions, files)
suite.toml     (which cases to run: boundary values, random cases, limits, executors)
        │
        ▼
mukoz check suite.toml  ──►  ACCEPT_WITHIN_SCOPE / HOLD / REJECT
                             + claims per property, counterexamples, scope and limitations (JSON)
```

Mukoz runs **enumerated cases**, not proofs. `ACCEPT_WITHIN_SCOPE` means every planned case
satisfied every property within the reported scope; it is not a guarantee about inputs outside
that scope. Anything Mukoz could not run or observe is reported as `HOLD`, never as success.

## Status: v0.1.0

| Host | Status |
|---|---|
| Linux x86-64 | Tier 1: the acceptance criteria in [docs/09 §9.8](docs/09-implementation-plan.md) pass (`tools/tier1.py`) |
| Others | Not built or tested |

What works on a Linux x86-64 host:

- **Targets:** x86-64 and AArch64 machine-code routines (SysV, AAPCS64, Apple arm64 ABI), static
  Linux ELF processes (x86-64, AArch64), Mach-O arm64 executables (`darwin-stdio/1`), functions of
  ELF relocatable objects, and programs split into modules (link files with boundary monitors).
- **Executors:** `emulated` (a separate emulator program, qualified per host by known-answer tests:
  `mukoz-emu-icicle` with icicle-emu by default, or `mukoz-emu` with Unicorn 2.1.1),
  `native-routine` and `native-process` (the real CPU and kernel, inside a sandbox, only where an
  owner policy allows), and differential tests between emulated and native execution.
- **Commands:** `check`, `show`, `replay`, `shrink`, `inspect`, `regressions`, `platform`, `expr check`.

Not implemented: macOS, Windows and Linux AArch64 hosts; dynamic linking; PE files; JSON-RPC.
The design documents describe more than is implemented; [docs/04 §4.10](docs/04-contract-language.md)
lists the implemented contract language exactly.

## Build

Requirements: Rust 1.97.1 (pinned in `rust-toolchain.toml`), CMake and a C compiler (Capstone is
built from source), and network access for the first build (icicle-emu is a git dependency
pinned to one commit).

```sh
cargo build --release      # target/release/mukoz and target/release/mukoz-emu-icicle
```

`mukoz` runs the emulator as a separate program: `$MUKOZ_EMU` if set, else `mukoz-emu-icicle`
next to the `mukoz` executable. Two programs implement the protocol:

| Program | Engine | License | Build |
|---|---|---|---|
| `mukoz-emu-icicle` ([emu-icicle/](emu-icicle/)) | icicle-emu (SLEIGH / p-code interpreter), the default | MIT OR Apache-2.0 | `cargo build --release` |
| `mukoz-emu` ([emu/](emu/)) | Unicorn 2.1.1 (QEMU-derived) | GPL-2.0-or-later | `cargo build --release -p mukoz-emu` (needs CMake) |

A plain `cargo build` contains no GPL code. To use Unicorn, set
`MUKOZ_EMU=target/release/mukoz-emu`. The engine qualification and every subject context record
which engine ran; [emu-icicle/README.md](emu-icicle/README.md) lists the known differences.
Without an emulator program, emulated results are `HOLD`
(`ENGINE_NOT_QUALIFIED: EMULATOR_UNAVAILABLE`); native executors still work.

`--no-default-features` builds without Capstone; only `show --disasm` changes.

## Quick start

```sh
# A correct 64-bit add routine (48 8d 04 37 c3: lea rax,[rdi+rsi]; ret)
./target/release/mukoz check examples/add64/suite.x86_64.toml
#   "admission": "ACCEPT_WITHIN_SCOPE", 4160 cases

# A mutant that subtracts
./target/release/mukoz check examples/add64/suite.x86_64.toml --artifact fixtures/x86_64/add64_mut_sub.bin
#   "admission": "REJECT", "reasons": ["VIOLATED: arith.add64/sum"]
#   findings[0]: inputs a = 0, b = 1, observed rax = 0xffffffffffffffff, counterexample cx-…

./target/release/mukoz show <counterexample-id>      # everything about one counterexample
./target/release/mukoz replay <counterexample-id>    # rerun it (e.g. after a fix, with --artifact)
```

Evidence is stored under `.mukoz/` in the current directory (`--store` to change it). The
counterexample becomes a regression case that every later check of the same contract runs first.
With `--gate` the exit code is the admission: 0 ACCEPT_WITHIN_SCOPE, 10 HOLD, 11 REJECT ([docs/07](docs/07-interface.md)).

More examples are in `examples/` (process contracts: `hello`, `todo`; memory regions: `copy`,
`checked_inc`, `strlen`; modules: `link_sum3`, `link_copy2`). For agents, start with [docs/11](docs/11-agent-guide.md).

## Native execution

`native-routine` and `native-process` run the subject on the host. They run only for artifacts
inside a trial zone declared in a `policy.toml` and only when the host probe confirms every
required isolation capability (user, network and PID namespaces, chroot, rlimits, seccomp for
routines). Without permission the result is `HOLD` (`NATIVE_NOT_PERMITTED`); Mukoz never falls
back from one executor to another. Read [docs/08](docs/08-security.md) before enabling it.

## Testing

```sh
cargo test --release          # unit, acceptance, negative and self-check tests
python3 tools/tier1.py        # the Tier 1 acceptance criteria, written to target/tier1-report.json
python3 selfcheck/run.py      # self-check stages 1–3 (docs/09 §9.6)
cargo build --release -p mukoz-emu && MUKOZ_EMU=$PWD/target/release/mukoz-emu \
  cargo test --release --workspace --exclude mukoz-kernels                # the same tests on Unicorn
```

## Documentation

[docs/README.md](docs/README.md) is the index: concept and guarantees, platform model,
architecture, contract language, execution, evidence and judgement, CLI, security, testing and
self-check, design decisions, agent guide, processes and modules.

## License

`mukoz` (this directory, except `emu/`) is licensed under either of the MIT license
([LICENSE-MIT](LICENSE-MIT)) or the Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your
option.

`mukoz-emu-icicle` ([emu-icicle/](emu-icicle/)), the default emulator program, is MIT OR
Apache-2.0, like `mukoz`. It embeds Ghidra processor specifications under the Apache License 2.0
([emu-icicle/sleigh/LICENSE](emu-icicle/sleigh/LICENSE), [emu-icicle/sleigh/NOTICE](emu-icicle/sleigh/NOTICE)).

`mukoz-emu` ([emu/](emu/)), the optional Unicorn program, links Unicorn and is licensed under the
GPL, version 2 or later ([emu/LICENSE](emu/LICENSE)). It is built only on request. It is a
separate program: `mukoz` starts it as a child process and talks to it over a documented line
protocol ([emu/PROTOCOL.md](emu/PROTOCOL.md)); neither links the other.

