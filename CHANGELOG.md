# Changelog

## 0.1.0 — 2026-10-05

First public version. Linux x86-64 host (Tier 1, docs/09 §9.8).

- Contract language: typed expressions over bitvectors, bools and byte strings; `requires`,
  `ensures`, frame conditions, bounded `forall` / `count` / `join`; allowed effects; termination.
- Bindings for x86-64 SysV and AArch64 (AAPCS64, Apple arm64) routines, memory regions with access
  rights, Linux processes (argv, stdin, files, stdout / stderr, exit status), Mach-O arm64
  executables, ELF relocatable-object functions, and link files with boundary monitors that
  assign blame between caller and callee.
- Case generation: boundary-value Cartesian product, random cases, weighted byte pieces, derived
  inputs, varied region alignment; regression cases stored per contract and target.
- Executors: `emulated` (a separate emulator program over the protocol `mukoz-emu/1`: icicle-emu
  in `mukoz-emu-icicle` by default, or Unicorn 2.1.1 in the optional GPL program `mukoz-emu`)
  gated by a per-host engine qualification;
  `native-routine` and `native-process` inside a probed sandbox, only within an owner's trial
  zone; differential tests between emulated and native execution.
- Evidence store, `show` (paged, optional disassembly), `replay`, `shrink`, `regressions`,
  `inspect`, `platform probe / show / qualify`, `expr check`; JSON output with a stable envelope.
- Self-check (docs/09 §9.6): Mukoz's own kernels checked as routines on both ISAs, the static
  CLI checked as a process, checker independence recorded in every assessment.
- Licenses: `mukoz` and `mukoz-emu-icicle` under MIT OR Apache-2.0 (the embedded Ghidra
  specifications under Apache-2.0); `mukoz-emu` (optional, built only
  with `-p mukoz-emu`) under GPL-2.0-or-later. A default build contains no GPL code.
