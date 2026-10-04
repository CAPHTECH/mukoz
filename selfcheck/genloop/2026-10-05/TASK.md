# Task

`todo.c` is supposed to satisfy the contract in `contract.toml` (bound to the program by
`binding.toml`; the case set is `suite.toml`). It does not yet. Make it pass.

- Build: `sh build.sh` (writes `todo.elf`).
- Check: `bin/mukoz check suite.toml --store .mukoz` — always use this same store.
- Investigate with `bin/mukoz show <id> --store .mukoz` and `bin/mukoz replay <id> --store .mukoz`
  (ids come from the check output), and `bin/mukoz --help`.
- Done when the check reports `"admission": "ACCEPT_WITHIN_SCOPE"`.

Rules: edit only `todo.c`. Do not edit the contract, binding or suite. Use only the files in this
directory and the output of `bin/mukoz`; do not look for Mukoz's source code, other fixtures or
reference solutions elsewhere on the machine.
