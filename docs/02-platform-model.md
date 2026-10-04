# 02 Platform model

Mukoz treats **the host on which Mukoz runs** and **the platform of the artifact being checked** on separate axes. Neither assumes a specific environment (Apple Silicon Mac, etc.). The first environment to run on is a Linux x86-64 host (§2.6).

## 2.1 Axes

| Axis | Example values | What it determines |
|---|---|---|
| ISA | `x86_64`, `aarch64` | Instruction semantics, register set, engine selection |
| Format | `raw`, `elf`, `macho`, `pe` | Inspector and loader |
| ABI | `sysv-x86_64`, `win64`, `aapcs64`, `apple-arm64`, `win-arm64` | Argument, return value, saved register, and stack rules |
| OS | `none`, `linux`, `darwin`, `windows` | Effect model (syscall adapter), whether native execution is possible |
| Executor | `emulated`, `native-routine`, `native-process`, `translated-process` | Execution method and observation/enforcement capabilities |
| Host | `linux-x86_64`, `linux-aarch64`, `macos-aarch64`, `macos-x86_64`, `windows-x86_64`, `windows-aarch64` | Which Executors are possible, and where results are recorded |

A **target platform** is a (ISA, format, ABI, OS) tuple, written like `x86_64/elf/sysv-x86_64/linux`. A raw routine sets the OS to `none`, as in `aarch64/raw/aapcs64/none`.

The "profile" of 0.4 (`aarch64-routine/1` etc.) is demoted to a **display name** derived from this tuple, the Executor, and the environment model. Verdicts and capability matching use the axis values.

## 2.2 Constraints on combinations

The axes are independent, but combinations have constraints. The constraint table is kept as data and not scattered through the code.

| Constraint | Example |
|---|---|
| Format and OS | `macho` goes with `darwin`, `pe` with `windows`, `elf` mainly with `linux`. `raw` goes with any |
| ABI and ISA | `sysv-x86_64` / `win64` go with `x86_64`; `aapcs64` / `apple-arm64` / `win-arm64` go with `aarch64` |
| ABI and OS | `apple-arm64` goes with `darwin`; `win64` / `win-arm64` go with `windows` |

A specification that violates a constraint is rejected at plan creation as `PLATFORM_COMBINATION_INVALID`.

## 2.3 Executor

| Executor | Condition | Main capabilities | Main limits |
|---|---|---|---|
| `emulated` | The engine for the target ISA is engine-qualified on that host (§2.5) | Observation and enforcement of registers, all memory accesses, and traps | Depends on the engine's instruction semantics. The OS is only what the effect model covers |
| `native-routine` | Host CPU ISA = target ISA | Return values, saved registers, and crash detection on the real CPU | Memory access protection is page-granular only. Byte-level monitoring is not possible |
| `native-process` | Host OS = target OS, and the host CPU can execute the target ISA | Real OS loader, real stdout/stderr, exit status | The absence of all syscalls, all memory accesses, and communication cannot be observed |
| `translated-process` | The host has a translation layer from the target ISA to the host ISA (qemu-user, Rosetta 2, x64 emulation on ARM Windows) | Same kind as `native-process` | Depends on the version and behavior of the translation layer. Recorded separately from real-hardware results |

- **No automatic fallback** between Executors. Mukoz does not run natively because emulation was unsupported.
- Translated execution is not equated with `native-process`. The evidence records the name and version of the translation layer.

## 2.4 Executability per host

`O` is possible, `-` is not possible, `T` is possible if a translation layer exists.

| Host \ Target | x86_64 routine | aarch64 routine | x86_64/elf/linux | aarch64/elf/linux | aarch64/macho/darwin | x86_64/macho/darwin | x86_64/pe/windows |
|---|---|---|---|---|---|---|---|
| `linux-x86_64` | emu, native | emu | emu, native | emu, T | emu | emu | emu |
| `linux-aarch64` | emu | emu, native | emu, T | emu, native | emu | emu | emu |
| `macos-aarch64` | emu | emu, native | emu | emu | emu, native | emu, T(Rosetta) | emu |
| `macos-x86_64` | emu, native | emu | emu | emu | emu | emu, native | emu |
| `windows-x86_64` | emu, native | emu | emu | emu | emu | emu | emu, native |

- `emu` means "if the engine supports it". What can actually be used is decided by the capability check on the host (§2.5).
- For process targets in the `emu` cells, this holds only when the effect model of that OS is implemented. The Windows effect model is out of the initial scope (chapter 05 §5.6).
- This table is a design-time expectation; nothing other than `linux-x86_64` has been tried.

## 2.5 Capability check and matching

The capabilities an Executor returns are **values checked on the host, not declared values**.

1. `mukoz platform probe` examines host information (OS version, CPU, CPU features) and the capabilities of each Executor, and stores them as a `HostProbe`.
2. `emulated` becomes "usable" only when it has passed the per-ISA engine qualification (an instruction test with known answers; chapter 09) on that host with that engine build. The record of passing is an `EngineQualification`.
3. Isolation capabilities (network cutoff, filesystem restriction, stopping descendant processes, etc.) are likewise returned only if they were actually tried and worked (chapter 08).

The Planner computes the capabilities needed for each required claim and matches them against the checked capabilities of the selected Executor.

```text
required:  detect all writes to memory outside the subject
selected:  native-routine (page-granular protection only)
result:    REQUIRED_CAPABILITY_UNAVAILABLE → that claim is NOT_EVALUATED → HOLD
```

A host being unable to execute the target (for example, running a Mach-O natively on Linux) is handled through the same path, and the reason is `HOST_CANNOT_EXECUTE_TARGET`. This is distinguished from being stopped by an OS execution policy (`EXECUTION_BLOCKED_PLATFORM_POLICY`).

## 2.6 Host support tiers

| Tier | Meaning |
|---|---|
| Tier 1 | Acceptance tests keep running on that host and pass |
| Tier 2 | The build and core tests pass. Some Executors are unverified |
| Tier 3 | Supported by design, but not built |

The initial state is Tier 3 for all. `linux-x86_64` becomes Tier 1 first (chapter 09 P1 to P4).

**Current state (2026-10-05):** `linux-x86_64` met acceptance criteria 1 to 9 of §9.8 in chapter 09 once with `tools/tier1.py`, and criterion 10 (generation loop) was recorded once. CI to "keep running" them is not configured `[U]`. The other hosts are Tier 3. The expected order is `macos-aarch64` next, then `linux-aarch64`, then `windows-x86_64`. The order is undecided and is placed in the open issues of chapter 10.

**Targets handled first on the current environment `linux-x86_64`:**

| Target | Executor | Stage |
|---|---|---|
| `x86_64/raw/sysv-x86_64/none` routine | `emulated`, `native-routine` | P1 |
| `aarch64/raw/aapcs64/none` routine | `emulated` | P2 |
| `x86_64/elf/sysv-x86_64/linux` static, no libc | `emulated` (Linux effect model), `native-process` | P3 |
| `aarch64/macho/apple-arm64/darwin` hello | `emulated` (Darwin effect model). native is `HOST_CANNOT_EXECUTE_TARGET` | P3 |

Host-dependent code is confined to the Executor and the Host probe. The Core (contract, plan, verdict, evidence) does not depend on the host's OS, CPU, or endianness. All values are read and written with an explicit byte order and do not depend on the host's byte order. However, big-endian hosts are not included in testing (no support tier is assigned).
