# Generation-loop trial, 2026-10-05 (docs/09 9.8 item 10)

An agent (a fresh Claude Sonnet subagent, no prior context) was given a working directory with
`todo.c` carrying six defects, the unchanged `examples/todo` contract, binding and suite (emulated
executor), the mukoz binary, and `TASK.md`. It was told to edit only `todo.c` and to use only that
directory and mukoz's output.

## Result (from the trial's `.mukoz` store, checked after the agent finished)

| run | admission | violated | planned | failed | regression cases run | counterexamples from regression cases |
|---|---|---|---|---|---|---|
| 1 | REJECT | add_appends_line, add_empty_fails, list_prints_db, clear_empties, usage | 532 | 483 | 0 | 0 of 15 |
| 2 | REJECT | same five (unchanged file, rerun: run 1 output was truncated by the agent's `head`) | 547 | 498 | 15 | 15 of 15 |
| 3 | REJECT | same five | 547 | 470 | 15 | 12 of 15 |
| 4 | REJECT | add_empty_fails, list_prints_db, clear_empties, usage | 550 | 454 | 18 | 12 of 12 |
| 5 | REJECT | list_prints_db, clear_empties, usage | 550 | 445 | 18 | 9 of 9 |
| 6 | REJECT | clear_empties, usage | 550 | 421 | 18 | 6 of 6 |
| 7 | REJECT | usage | 550 | 402 | 18 | 3 of 3 |
| 8 | ACCEPT_WITHIN_SCOPE | — | 550 | 0 | 18 | — |

- 8 check runs (6 with an edit), REJECT → ACCEPT_WITHIN_SCOPE.
- The diff from `todo.initial.c` to `todo.final.c` fixes exactly the six seeded defects; contract,
  binding and suite were compared unchanged after the trial.
- **Regression cases catching a recurrence: 0.** No fixed property became violated again, so the
  regression set had no recurrence to catch. From run 2 on, the first counterexamples of every
  still-open property came from regression cases (the table's last column).
- Not verified: that the agent read nothing outside the directory (its own report says so; the
  transcript was not audited).

## What the agent found lacking, and what changed

- Regression case ids were renumbered each run (`reg-00007` then `reg-00009`). Fixed: a regression
  case is `reg-<stored digest>`, and it keeps its stored filler seed (it previously drew a new one,
  so it was not the same execution as the counterexample it came from — docs/04 4.8).
- Findings were capped at three, so with five violated properties `usage` had no inline finding.
  Fixed: one inline finding per violated property.
- Not changed: only the first counterexample of each property is inline; the rest are reached
  through each claim's counterexample ids and `mukoz show`.

These fixes came after the trial; the trial ran the earlier behaviour.
