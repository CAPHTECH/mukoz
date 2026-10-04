# eval/todo — a command-line to-do list as a Linux process (x86-64 raw)

- `spec.md` — the task given to the participant.
- `contract.toml` / `binding.toml` / `suite.toml` — the Mukoz suite (condition A). The list file is a derived input (`generate.vars.db.expr`), so every generated file is well-formed.
- `ref/todo.c`, `ref/build.sh` — reference (gcc, freestanding) and 5 mutations → `bin/*.bin`.
- `launch.c` — native launcher (`gcc -O2 -o launch launch.c`; run under `unshare -Urn`): maps the image as the spec says, chroots into a directory, seccomp allows only the spec's system calls.
- `oracle_todo.py` — hidden oracle: random multi-command sessions run natively, compared with an independent Python model of the spec.

Checked once each (2026-10-04): reference ACCEPT / PASS; mut_lastid, mut_full49, mut_leadzero, mut_clearcreate REJECT / FAIL;
mut_zeropad ACCEPT / PASS on both (the stack it leaves unzeroed is fresh, hence zero, in both environments — equivalent here).
