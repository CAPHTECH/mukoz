# mukoz-emu-icicle

A second implementation of the `mukoz-emu/1` emulator program ([../emu/PROTOCOL.md](../emu/PROTOCOL.md)),
built on [icicle-emu](https://github.com/icicle-emu/icicle-emu), a SLEIGH / p-code interpreter.
It shares no code with Unicorn or QEMU. `mukoz` uses it when `MUKOZ_EMU` points at it:

```sh
cargo build --release
MUKOZ_EMU=target/release/mukoz-emu-icicle target/release/mukoz check examples/add64/suite.x86_64.toml
```

The engine qualification runs against whichever program `mukoz` talks to; its identity
(`icicle-emu git 3292602fd485 … via mukoz-emu-icicle 0.1.0`) is part of every subject context.

## License

This program is licensed under either of the MIT license or the Apache License 2.0, at your
option (the repository's [LICENSE-MIT](../LICENSE-MIT) and [LICENSE-APACHE](../LICENSE-APACHE)).
icicle-emu is MIT OR Apache-2.0 (its crates carry no `license` field; the repository ships
LICENCE-MIT and LICENCE-APACHE and its README states the dual license). Every other dependency is
under permissive licenses (`cargo metadata`; `r-efi` offers MIT or Apache-2.0 among its options).
The processor specifications in [sleigh/](sleigh/) are 38 files
copied unchanged from the icicle-emu fork of Ghidra (commit
`50230050fa58bd40d5a96cab9c167fc55bc92a76`, the commit icicle-emu's Dockerfile pins) and are
licensed under the Apache License 2.0 ([sleigh/LICENSE](sleigh/LICENSE),
[sleigh/NOTICE](sleigh/NOTICE)). They are embedded in the binary, so a binary distribution
carries the Apache-2.0 license and the NOTICE.

## How the protocol maps onto icicle

- Each lifted block is instrumented. After every instruction marker, a hook checks `until`,
  code ranges, frozen registers, the budget and the time limit, records the ring and reports
  breakpoints. Before every RAM load and store, a hook checks the allowed ranges with the
  access's full address and width. Accesses are checked before they happen.
- System calls, interrupts and faults end `Vm::run` with an exception. The event is served,
  then execution resumes after the trapping instruction.
- `rflags` (x86-64) and `nzcv` (AArch64) are assembled from SLEIGH's separate flag registers.
- The JIT and all p-code optimisation are off, so register values are exact at every hook.
- There is one engine per ISA, reset before every run. The SLEIGH specifications are compiled
  once at startup from a private temporary directory, which is removed again.

## Differences from mukoz-emu (Unicorn)

| Case | mukoz-emu (Unicorn 2.1.1) | mukoz-emu-icicle |
|---|---|---|
| x86 `popcnt` | Not implemented (invalid instruction) | Implemented. The qualification accepts either result, but never a wrong value |
| A64 `sdiv` of MIN by −1 | MIN | icicle raises a division exception. This program writes MIN (the architectural result) and continues |
| Self-modifying code (changing bytes that already ran) | Executes the new bytes | Engine error `SELF_MODIFYING_CODE`, so `ENGINE_ERROR` and HOLD |
| An x86 instruction icicle cannot execute | — | Engine error `UNIMPLEMENTED_OP`, so HOLD. A64 reports it as interrupt 1, like Unicorn |
| A data access that fails after passing the allowed check (unmapped or protected) | `fault` with the faulting access | `fault` with the last checked access |

## What was verified (linux-x86_64 host, one run each)

- The known-answer qualification passes on both ISAs: x86-64 533 of 533 vectors, AArch64 324 of
  324. Before the `sdiv` workaround, one AArch64 vector failed.
- With `MUKOZ_EMU` pointing at this program, `cargo test --release` and `python3 tools/tier1.py`
  pass.
- Fault injection was run twice:
  - With the allowed-range check disabled, two acceptance tests fail.
  - With the jump-after-break handling disabled, its protocol test fails.
- `tests/protocol.rs` (6 tests) also passes against `mukoz-emu`. The same file is in
  `emu/tests/`.

Not verified:

- SIMD, floating-point, atomic and system instructions beyond what the fixtures use.
- Performance on large suites.
- Hosts other than linux-x86_64.
