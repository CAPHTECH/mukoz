# eval/todo_mod — the todo task (../todo/spec.md) built from 8 modules

- `spec.md` — the participant's task: main + 7 routines (parse_id, udec, find_rec, fmt_line, make_rec, del_rec, clear_done), linked at fixed bases with an import table at 0xf0000.
- `make.py <dir>` — writes the Mukoz files: `check/<routine>/` (routine contracts, bindings, suites), `check/main/` (the process suite, contract = ../todo/contract.toml, with monitors on every routine) and `link.toml`.
- `ref/` — reference modules in C (`build.sh`; routines are built position-independent) and mutations → `bin/`.
- `flatten.py` — one image from 0xf0000 for `../todo/launch -b f0000`; `judge_mod.py` — oracle + Mukoz on a submission.

Checked once (2026-10-05): reference ACCEPT on all 8 suites and oracle PASS. Mutations: parse_id_mut_leadzero REJECT alone and `link.parse_id.parse_id.ensures` (blame callee) in main, oracle FAIL; udec_mut_nul REJECT alone and `link.udec.udec.ensures` in main, oracle PASS (the extra byte is overwritten — a contract violation with no visible effect); find_rec_mut_16 REJECT alone, ACCEPT in main and oracle PASS (no colliding ids in those inputs); main_mut_deln (removes the next record) `link.del_rec.del_rec.requires` (blame caller) and app.todo/rm, oracle FAIL.
