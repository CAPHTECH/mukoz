# Check accesses that leave no final-state trace

Mukoz can check **which memory a call is allowed to read or write**, even when its returned
value and final data look correct. This example copies one 64-bit value into an output buffer.
The caller lends the source for reads and supplies a write-only audit buffer.

The contract checks the copied bytes and status. The binding also sets:

```toml
[regions.src]
size = "bv64(8)"
init = "le_bytes(input.value)"
access = "r"

[regions.audit]
size = "bv64(8)"
init = 'hex"43524544454e544c"'
access = "w"
```

These permissions describe the current call. The audit buffer exists and has initialized data,
but this call may only write it. Reading its previous contents violates the binding.

| Routine | Extra action before copying | `mukoz check` |
|---|---|---|
| `correct` | None | `ACCEPT_WITHIN_SCOPE` |
| `read_audit` | Read one byte of the write-only audit buffer | `REJECT`: forbidden read |
| `restore_source` | Invert one source byte, then restore it | `REJECT`: forbidden write |
| `read_past_source` | Read one byte just past the caller's eight-byte input view | `REJECT`: read outside the allowed range |
| `scratch` | Use permitted stack scratch space | `ACCEPT_WITHIN_SCOPE` |

The raw binaries are included for x86-64 and AArch64. No source code, compiler, debug information,
Memcheck, native execution or VM is needed to check them:

```sh
cargo build --release
./target/release/mukoz check examples/security_access/suite.x86_64.toml
./target/release/mukoz check examples/security_access/suite.x86_64.toml \
  --artifact examples/security_access/x86_64/restore_source.bin
./target/release/mukoz check examples/security_access/suite.aarch64.toml \
  --artifact examples/security_access/aarch64/read_audit.bin
```

The failing property is `machine.memory.access`. A finding identifies the instruction offset,
read/write direction, address, width and the region's permission, with a counterexample ID.
For example, `restore_source` reports a one-byte write inside `src`, which does not allow writes.
Use the ID from the check with `mukoz replay <id>` to rerun it. The checked run stops at the
forbidden access; its unfinished output checks remain `INCONCLUSIVE`.

Changing `src` from `r` to `rw`, or `audit` from `w` to `rw`, permits the corresponding operation
by the same binary. The regression tests cover that policy change as well as the permitted
stack access.

## What the comparison establishes

The [X3 experiment](../../research/security-observation/x3/README.md) runs the same routines
with mapped, accessible data before applying call permissions. Across 24 inputs per ISA,
all five variants have identical final general registers, flags, data-page contents and
stack-page contents. The permission checks expose the intermediate accesses.

On an x86-64 host, a separate Memcheck control puts the three words in one live allocation.
All five routines produce the expected output without a Memcheck error for the three tested
inputs. They are allocator-valid operations; Mukoz's binding expresses the caller's narrower
access policy. This illustrates the extra question the contract can answer.

The scope is these synthetic routines and enumerated inputs. The caller supplies the access
policy; Mukoz does not infer an application's authorization rules.

To rebuild the fixtures, use `python3 examples/security_access/build.py` with GCC,
`aarch64-linux-gnu-as` and Rust's `llvm-objcopy` available. `manifest.json` records the raw
artifact hashes. Assembly sources are supplied for inspection and rebuilding.
