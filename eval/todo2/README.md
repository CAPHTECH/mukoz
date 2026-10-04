# eval/todo2 — to-do CLI with priorities, find, edit, import from stdin (x86-64 raw process)

- `spec.md` — the task. `oracle_todo2.py` — hidden oracle (random sessions run natively with ../todo/launch, compared with a Python model).
- `ref/todo2.c`, `ref/build.sh` — reference (gcc, 3435 bytes) and 5 mutations → `bin/`.
- No Mukoz suite yet. Checked once (2026-10-04, seed 7): reference PASS; mut_stable, mut_case, mut_importlast, mut_errorder, mut_emptyimport FAIL.
