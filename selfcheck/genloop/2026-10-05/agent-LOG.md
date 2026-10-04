# LOG

Note: run 1 was invoked piped through `head -150` (output truncated, so I re-ran it as run 2 to get the full JSON). Both runs were on the unmodified todo.c. Run 2 already contained `reg-` cases (created from run 1's counterexamples).

## Iter 1-2 (baseline, unmodified todo.c)
- Change: none.
- CLI output: REJECT; violated add_appends_line, add_empty_fails, list_prints_db, clear_empties, usage. Run 2: 15 regression cases run (reg-*), 498/547 failed.
- Findings seen (all reg- from the run-1 counterexamples):
  - reg-00003 add_appends_line: add "d" on empty db -> db "64", expected "640a". Observed syscalls: open(...,0x41,0644), write 1 byte, close. (no newline written)
  - reg-00007 add_empty_fails: add "" -> exit 0, stderr "empty\n", expected code 1 (and err unspecified for this property).
  - reg-00002 list_prints_db: list with no db -> exit 1 (open = -2), expected code 0 with empty out.

## Iter 3
- Change: added `sys3(1, fd, "\n", 1)` after writing the text in `add` (a newline was missing). Led by reg-00003's observed db "64" vs expected "640a" (why_false / last_syscalls).
- Result: REJECT; still violated all 5 properties (cases_failed 498 -> 470). add_appends_line now reported on a non-reg case bnd-00216: before.db "65", add "\"" -> after.db "220a" (expected "65220a"); open flags 0x41 (no O_APPEND) overwrote at offset 0. reg-00007 (add_empty_fails) and reg-00002 (list_prints_db) still failing (reg- failures: reg-00007, reg-00002; unchanged, not yet addressed).

## Iter 4
- Change: ADD_FLAGS gained O_APPEND (02000). Led by bnd-00216 observed open flags 0x41 and db "220a" instead of "65220a" (before.db was overwritten from offset 0).
- Result: REJECT; add_appends_line no longer violated. Still violated: add_empty_fails, list_prints_db, clear_empties, usage. reg- failures: reg-00009 (add_empty_fails), reg-00003 (list_prints_db), reg-00001 (clear_empties). (reg- ids are renumbered between runs; same inputs as before.) 

## Iter 5
- Change: `add ""` now exits 1 instead of 0. Led by reg-00009 (add_empty_fails): observed exit 0 with stderr "empty\n", contract wants code 1.
- Result: REJECT; add_empty_fails no longer violated. Still violated: list_prints_db, clear_empties, usage. reg- failures: reg-00003 (list_prints_db), reg-00001 (clear_empties), reg-00002 (usage; first time shown in my summary: bogus/argc-0 case writes "usage\n" to stdout, exit 2).

## Iter 6
- Change: `list` on a missing db now exits 0 (prints nothing) instead of 1. Led by reg-00003 (list_prints_db): observed open(...) = -2 then exit(1); contract wants code 0 and empty out.
- Result: REJECT; list_prints_db no longer violated. Still violated: clear_empties, usage. reg- failures: reg-00001 (clear_empties), reg-00002 (usage).

## Iter 7
- Change: `clear` open flags gained O_TRUNC (01000). Led by reg-00001 (clear_empties): observed open flags 0x41, db stayed "65" (not emptied).
- Result: REJECT; only usage violated now. reg- failure: reg-00002 (usage): usage text goes to stdout (write fd 1), contract wants stderr (result.err == "usage\n").

## Iter 8
- Change: "usage\n" now written to fd 2 (stderr) instead of fd 1. Led by reg-00002 (usage).
- Result: ACCEPT_WITHIN_SCOPE, no violated properties, 0/550 cases failed, 18 regression cases run, none failed.

Total check runs: 8 (incl. the truncated first run on the unmodified file).
