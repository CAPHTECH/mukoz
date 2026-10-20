# Four security checks beyond memory access

These examples check the finished binary's calling convention, module boundaries, integer
argument handling and permitted effects. Each family includes a permitted control. The raw
fixtures run on Mukoz's emulated executor; the examples need no native execution or VM.

```sh
cargo build --release
python3 examples/security_checks/run.py /tmp/mukoz-security-examples
```

Use a fresh output directory. The runner checks both x86-64 and AArch64, validates all planned
cases, and replays each announced violation. The prior expectations are in `cases.json`.
Each check's command, full JSON result, evidence store and replay result are saved separately.

| Family | Violation | Control and useful distinction |
|---|---|---|
| ABI | Clobber a callee-saved register | Saving and restoring it passes. The direct violating routine still computes the correct sum on every case. |
| Module boundaries | Break a callee's result or ABI, or call it outside its precondition | Correct modules pass. Evidence assigns the violation to the caller or callee. The precondition example still computes the correct final sum. |
| Integer arguments | Depend on unspecified upper register bits; zero-extend a value that requires sign extension | Explicit zero/sign extension passes. A typed `bv32` input does not give the routine a promise about the upper 32 bits of a 64-bit argument register. |
| Effects | Attempt a system call from a pure routine; write output when the process contract forbids it | The same process binary passes with `write` permitted and is rejected with it forbidden. |

The matrix also includes unsupported instructions as a negative control: they produce
`HOLD` with `UNSUPPORTED_DURING_RUN`, rather than a fabricated forbidden-effect violation.

## Run individual checks

A correct result can still corrupt the caller's saved register:

```sh
./target/release/mukoz check examples/security_checks/sum.x86_64.suite.toml \
  --artifact fixtures/x86_64/add64_mut_clobber_rbx.bin
# REJECT: machine.abi.callee_saved; arith.add64/sum remains satisfied
```

A caller can get the right result while violating its callee's documented precondition:

```sh
./target/release/mukoz check examples/security_checks/module.strict.x86_64.suite.toml
# REJECT: link.arith.add64.requires; blame=caller; arith.sum3/sum remains satisfied
```

A replacement module can violate its result contract:

```sh
./target/release/mukoz check examples/security_checks/module.x86_64.suite.toml \
  --module arith=fixtures/x86_64/add64_mut_sub.bin
# REJECT: link.arith.add64.ensures; blame=callee
```

For a 32-bit input interpreted as signed, `0xffffffff` must become `0xffffffffffffffff`:

```sh
./target/release/mukoz check examples/security_checks/signed.x86_64.suite.toml
# ACCEPT_WITHIN_SCOPE
./target/release/mukoz check examples/security_checks/signed.x86_64.suite.toml \
  --artifact examples/security_checks/x86_64/zero_extend.bin
# REJECT: arith.sext32/sign_extended
```

One process binary under two explicit effect contracts:

```sh
./target/release/mukoz check examples/security_checks/output.allowed.aarch64.suite.toml
# ACCEPT_WITHIN_SCOPE
./target/release/mukoz check examples/security_checks/output.denied.aarch64.suite.toml
# REJECT: effects.no_forbidden
```

A rejected check provides a counterexample ID. Run `mukoz replay <id>` with the same store
to reproduce that property and its caller/callee attribution.

## Scope and regression tests

The check covers the enumerated inputs, the supplied contracts and the qualified engine.
The fixtures are small synthetic routines and modeled processes. The effects example observes
system-call attempts within the model; it does not establish a real application's network policy.
ABI checks cover the calling convention obligations implemented by Mukoz, not complete control-flow
integrity. Sign extension is required explicitly by the contract.

```sh
cargo test --release --test security_checks
python3 research/security-observation/restore_evidence.py
python3 examples/security_checks/test_verifier.py
```

The Rust test runs the complete matrix and counterexample replays. The Python negative tests
alter recorded claims, blame, qualification, coverage and evidence IDs to ensure the verifier
rejects false success. The recorded results are archived under
[research/security-observation/evidence](../../research/security-observation/evidence/README.md).

The examples reuse existing ABI and module fixtures. To rebuild the new argument and system-call
fixtures, use GCC, an AArch64 assembler and Rust's llvm-objcopy:

```sh
python3 examples/security_access/build.py --directory examples/security_checks
```

`manifest.json` records the raw artifact hashes.
