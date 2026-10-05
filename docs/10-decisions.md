# 10 Design decisions, open issues and references

## 10.1 Design decisions

| ID | Decision | Reason | Relation to 0.4 |
|---|---|---|---|
| ADR-01 | The correct answer is determined from the Contract | If expected values are derived backward from the generator's explanation or the subject's output, errors become shared | Inherited (D01) |
| ADR-02 | Separate the Contract (ISA-independent) from the Binding (platform-dependent) | The same contract can be reused across multiple ISAs and multiple hosts | Inherited (D02). Raised in importance as the basis for multiple ISAs |
| ADR-03 | Represent the platform with independent axes: ISA, format, ABI, OS, Executor and Host. Profile names are for display | Requirement to support hosts and targets other than ARM Mac, both | Changed (replaces 0.4's D08 "initially limited to AArch64 and Mach-O") |
| ADR-04 | Split identity into subject context and execution platform. The latter is the scope of a claim | If the host is used as a matching key, all results from other hosts become invalid; if it is left out, host differences are not retained | Changed |
| ADR-05 | No automatic fallback between Executors. Use only capabilities confirmed on the host | Declared values and fallback silently change the scope | Inherited (D09), generalized |
| ADR-06 | The first operating environment is Linux x86-64 | It is the current development environment. A design that cannot be verified here cannot move forward | Changed (0.4 assumed a real Apple Silicon machine) |
| ADR-07 | Implementation language is Rust | Memory safety for parsing untrusted input. `no_std` and `extern "C"` allow building runtime-independent functions, which can be made subjects of self-check. Builds for multiple OSes and ISAs | Inherited. Reason added |
| ADR-08 | The human-written format is TOML and the source of truth is JSON. Expressions are written as strings and stored after conversion to a typed AST | No need for countermeasures against YAML aliases, tags and implicit type conversion. Expressions can be written short | Changed (0.4 used YAML + JSON AST) |
| ADR-09 | Add variable-length bytes and bounded quantifiers to expressions | To make it possible to write contracts for routines that handle buffers | Added |
| ADR-10 | Add process-boundary contracts (stdout, stderr, exit) | To make it possible to write, as a contract, the expectations for an executable such as hello | Added |
| ADR-11 | Split the effect model into "the meaning of effects" and "OS×ISA syscall adapters" | Linux and Darwin share the meaning, and Windows can be added with a separate adapter (API stub) | Changed |
| ADR-12 | Add native-routine on a same-ISA host | A cheap way to get a real CPU as an independent source of expected values | Added |
| ADR-13 | The MVP interface is a CLI with JSON output only. JSON-RPC, MCP, receipt and HMAC come later | Do not build peripheral machinery before running tests | Changed (0.4 inherited JSON-RPC and 12 operations) |
| ADR-14 | Perform self-check and record checker independence in the claim. `self` alone gives HOLD by default | A checker with the same error will miss that error | Added |
| ADR-15 | `release_authorized` is always false | A test pass/fail and permission to release are separate judgements | Inherited (D10) |
| ADR-16 | Take FSL integration and formal verification (bounded verification, refinement) out of the scope of this document set. Leave only reserved `method` and evaluation values in the data model | By instruction. First make finite testing reliable | Changed |
| ADR-17 | The artifact to check is specified by the Suite's `[artifact]` or the CLI's `--artifact`, and is not written in the Binding. The default for a raw code region is the whole file | So that repetition is possible without rewriting the contract or Binding, even when the AI changes the file or instruction length on each generation | Changed (0.4 used the Binding's `target_id`) |
| ADR-18 | Accumulate counterexamples as regression cases tied to the contract digest and target, and run them first in the next check. They are not deleted automatically | To confirm recurrence on every iteration of generation | Added |
| ADR-19 | Attach diagnostics (failure point, recently executed instructions, violating access, subexpression values) to findings. They are not used for the verdict | Clues for the AI to fix machine code. Do not mix diagnosis with verdict | Added |
| ADR-20 | `--fail-fast` can stop after the first counterexample. REJECT is valid and ACCEPT is not possible | Speed of iteration | Added |
| ADR-21 | Native execution is allowed, in addition to permission by digest, in a trial zone set by the owner where all required isolation capabilities can be confirmed. Disabled by default | So that iteration does not stall on a human's permission for every generation. It is a trade-off with safety, so the owner chooses | Added |
| ADR-22 | The purpose of development is "AI agents can generate and fix binaries that satisfy a contract, without source", and the first exploration goal is a comparison trial of hypotheses about that value. Tier 1 is a stage goal | To measure success by value, not by the completeness of the tool. The final verdict is not left to Mukoz | Added |
| ADR-23 | The main use is (i) direct generation of machine code by AI. (ii) Checking distributed artifacts is limited to what can be done with native-process on the same OS, with no dedicated investment | Mukoz's own value lies in routine-level checking, monitoring and counterexamples. Most of (ii) can be covered by ordinary CI | Added. To be re-decided after the comparison trial |
| ADR-24 | Unicorn (GPL-2.0) runs only in the separate program `mukoz-emu` (GPL-2.0-or-later). `mukoz` is MIT OR Apache-2.0 and does not link it; the two talk over the documented line protocol `mukoz-emu/1`, in which the emulator knows nothing about contracts | `mukoz` can be used and embedded under a permissive license while the emulator stays GPL. The boundary is a generic machine interface (memory, registers, run, events), not shared internal data structures [R]: the usual reading is that programs communicating this way are separate works, but this has not been reviewed by a lawyer `[U]` | Added 2026-10-05. Cost: the test files took between 0% and about 20% longer than with the engine linked in (one run each). A second program, `mukoz-emu-icicle` (icicle-emu, MIT OR Apache-2.0), implements the same protocol, so the emulated executor can run with no GPL code at all |

## 10.2 Open issues

| Issue | Current proposal | What is needed to decide |
|---|---|---|
| Unicorn build and operation | First candidate | The P1-0 spike. Alternatives if it fails: a small instruction interpreter of our own (limited set of supported instructions), or a different emulator |
| Mukoz license | Decided (ADR-24) | Whether the process boundary is enough under the GPL has not been reviewed by a lawyer `[U]` |
| Next host to make Tier 1 | macOS arm64 → Linux arm64 → Windows x86_64 | Whether users and CI environments exist |
| How to build aarch64 fixtures | `binutils-aarch64-linux-gnu` or the Rust aarch64 target | Whether packages can be installed |
| Mach-O hello fixture | Obtain 0.4's `hello-arm64`, or assemble it by hand | Where the file is (it is not in this directory) |
| Delegation of cgroups | If usable, use for stopping descendants and memory limits | Confirmation on the current host `[U]` |
| Windows effect model | Stubs per API (kernel32 / ntdll) | Design of contract-bearing stubs, a Windows host |
| Checking the ABI tables and syscall numbers against primary sources | The tables in chapter 05 are not yet checked `[U]` | At implementation time, open each source and pin the version |
| Compatibility with old Mukoz v0.3 | Compatibility is not assumed | Where the old implementation and schema are |
| JSON-RPC / MCP | Later | After the CLI operations are settled |
| Stub generation from contracts (module splitting) | Not implemented. To check only a caller without implementing the callee, plug in a separate implementation known to be correct with `--module` (chapter 12) | A way to build call responses from the contract's ensures (for a functional contract, expression evaluation is enough; for a contract that states only relations, a search for solutions is needed) |
| Callee overwriting caller memory | Not detected. Boundary monitoring looks only at the callee contract's values and the ABI, and whole-process memory monitoring looks only at "is it an allowed region" | A monitor that narrows the allowed range, only during the call, to the regions of the callee's contract |
| ELF module splitting and dynamic linking | Out of scope (static ET_EXEC only) | Whether Mukoz takes on relocation and symbol resolution |
| Preventing tampering with contracts and Suites | **Not provided for now** (instruction of 2026-10-04). If the AI loosens an ensures or reduces the cases in a Suite, the result can be ACCEPT. At present, the only safeguard is that the contract and Suite digests remain in the evidence, so it can be noticed afterwards | When it becomes necessary: pin in the policy the digests of Contracts and Suites approved by the owner, and make a mismatch HOLD (equivalent to 0.4's approved contract) |

These do not block starting P0 (types, expressions, verdicts, Store).

## 10.3 Shortcuts not taken

- Building expected values with the same logic as the generator
- Treating successful disassembly as success
- Returning a success response to an unsupported syscall, or treating an unsupported instruction as a NOP
- Ending at the first `ret`
- Setting all initial registers to 0 and regarding that as an ABI guarantee
- Treating effects not observed in native execution as "did not happen"
- Treating a timeout as success
- Silently running natively what does not run under emulation
- Equating before and after signing, or reusing evidence based only on a match of `.text`
- Aggregating results obtained on different hosts without recording the hosts
- Treating the result of self-check on a par with an independent check
- Calling a test report a certificate

## 10.4 References

**Not opened and re-confirmed** when this document was written (2026-10-04). This is a list of the sources 0.4 referenced and sources newly listed in this document. Pin the versions and confirm at implementation time.

| ID | Source | Use |
|---|---|---|
| R1 | Apple, *Writing ARM64 code for Apple platforms* — <https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms> | `apple-arm64` ABI |
| R2 | Arm, *Procedure Call Standard for the Arm 64-bit Architecture (AAPCS64)* — <https://github.com/ARM-software/abi-aa/blob/main/aapcs64/aapcs64.rst> | `aapcs64` ABI |
| R3 | *System V Application Binary Interface, AMD64 Architecture Processor Supplement* | `sysv-x86_64` ABI |
| R4 | Microsoft, *x64 calling convention* / *ARM64 ABI conventions* (Microsoft Learn) | `win64`, `win-arm64` ABI |
| R5 | Unicorn — <https://github.com/unicorn-engine/unicorn> | Emulation, hooks, license |
| R5b | icicle-emu — <https://github.com/icicle-emu/icicle-emu>; M. Chesser et al., *Icicle: A Re-designed Emulator for Grey-Box Firmware Fuzzing* (arXiv:2301.13346) | Second emulator program |
| R6 | Capstone — <https://www.capstone-engine.org/> | Disassembly |
| R7 | `object` crate — <https://docs.rs/object/latest/object/> | Reading formats |
| R8 | Apple XNU `EXTERNAL_HEADERS/mach-o/loader.h` — <https://github.com/apple-oss-distributions/xnu> | Mach-O structure |
| R9 | Apple XNU `bsd/kern/syscalls.master` — same as above | Darwin syscall numbers |
| R10 | Linux kernel `arch/x86/entry/syscalls/syscall_64.tbl`, `include/uapi/asm-generic/unistd.h` | Linux syscall numbers |
| R11 | *ELF-64 Object File Format* / System V gABI | ELF structure |
| R12 | Microsoft, *PE Format* | PE structure |
| R13 | Apple, *TN2206: macOS Code Signing In Depth* | Signing and modification |

The previous version 0.4-design-draft.1 (`Mukoz_Binary_Test_Tool_Design.md`) was deleted after its content was moved into this document set (2026-10-04). The FSL integration, formal verification and the migration table from old Mukoz v0.3 were discarded without being moved.

Internal materials of old Mukoz v0.3 that 0.4 referenced (not in this directory): `DESIGN(6).md` (v0.3.0 design), `PROTOCOL.md` (`mukoz/1` communication specification), `NAMING(1).md` (naming conventions), `README(20261001-202535).md` (prototype status. States that build/test of the old Rust implementation is unverified).
