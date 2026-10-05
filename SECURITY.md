# Security

## Reporting a vulnerability

Report vulnerabilities privately through GitHub: **Security → Report a vulnerability** on this
repository. Do not open a public issue for a security problem.

## What Mukoz runs

Mukoz executes the artifacts it checks. Read [docs/08](docs/08-security.md) before checking
artifacts you do not trust.

- **`emulated`** (the default) runs the subject in an emulator, in a separate process. The
  emulator boundary is meant for correctness and crash isolation. It is not a sandbox that has
  been reviewed against malicious code.
- **`native-routine`** and **`native-process`** run the subject on the real CPU and kernel.
  Mukoz runs them only when an owner's `policy.toml` allows the artifact, through a trial zone
  or an explicit digest. The host probe must also confirm every isolation capability that the
  policy entry requires. These capabilities are user, network and PID namespaces, chroot,
  resource limits, and seccomp for routines. A digest entry may require none; the policy owner
  decides. Without permission the result is `HOLD` (`NATIVE_NOT_PERMITTED`).
- Output of the subject (stdout, stderr, files) appears in Mukoz's JSON output as data. Tools
  and agents that read that output should treat those fields as untrusted. Marking them is
  designed (docs/07 §7.3) but not implemented in 0.1.0.

Issues in scope include:

- a way for a subject to escape these boundaries or to affect the host;
- a way for a subject to make Mukoz report `ACCEPT_WITHIN_SCOPE` for behaviour it did not
  observe;
- a way to tamper with the evidence store.
