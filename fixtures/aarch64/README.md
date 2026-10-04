AArch64 fixtures are hand-encoded bytes (no AArch64 assembler on the build host).
- add64.bin: `add x0, x0, x1; ret` (8b010000 d65f03c0)
- add64_mut_sub.bin: `sub x0, x0, x1; ret` (cb010000 d65f03c0)
Encodings are from docs 0.4 / hand encoding; behaviour was checked once under Unicorn (docs/devlog.md), not with a disassembler.
