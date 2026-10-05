# eval — comparison-trial material

This directory holds the material for the comparison trial in [docs/10](../docs/10-decisions.md)
(ADR-22). The trial asks whether an AI agent that writes machine code without source produces
correct code more often when it checks its work with Mukoz than when it does not. It is research
material, not part of the tool: `mukoz` does not read anything here. The only exception is one
acceptance test, which uses `tasks/smax/` as an ordinary suite.

Each run is judged by a **hidden oracle**. The oracle shares no code with Mukoz: it has Python
reference implementations written by hand and a C runner that executes the submission natively.
It draws its cases from a seed chosen after the submission exists.

## Layout

| Path | What it is |
|---|---|
| `tasks/<task>/` | One routine task: `spec.md` (what the participant is given) and the Mukoz suite for it (`contract.toml`, `binding.toml`, `suite.toml`). Tasks ending in `_a64` are AArch64 versions |
| `make_tasks.py` | Generates `tasks/` (the contracts are written by hand inside it) |
| `oracle/` | The hidden oracle: `oracle.py` (x86-64, native), `a64.py` (an integer A64 interpreter written from the Arm pseudocode, for AArch64 tasks; anything outside its subset is reported as unsupported) and `runner.c` (seccomp-strict child per case, guard pages, canaries, callee-saved register check) |
| `reference/` | Reference solutions in assembly (`*.s`), with mutations (`*_mut_*.s`) used to check that both Mukoz and the oracle reject them. `build.sh` assembles them into `bin/`; `gcc/` holds a gcc-compiled reference |
| `judge_all.py` | Judges every submission in an experiment directory with the oracle (3000 cases) and records Mukoz's own verdict on the final file. Writes `results.jsonl` |
| `summarize.py` | Summarizes `results.jsonl` per condition: oracle pass rate, and cases where Mukoz accepted but the oracle failed |
| `todo/` | A command-line to-do list as a Linux process (x86-64 raw): spec, Mukoz suite, reference and mutations, native launcher, oracle |
| `todo2/` | A larger to-do task (priorities, find, edit, import). Spec, reference, mutations and oracle; no Mukoz suite yet |
| `todo_mod/` | The same to-do task built from 8 modules linked at fixed bases, checked with boundary monitors |

## Running

An experiment directory contains one subdirectory per run, named `<condition>-<task>` (for
example `A-base64`). The participant writes its submission to `solution.bin` there. Condition `A`
runs may use Mukoz, and their evidence store `.mukoz/` is counted. Then:

```sh
gcc -O2 -o eval/oracle/runner eval/oracle/runner.c
python3 eval/judge_all.py <experiment-dir> target/release/mukoz
python3 eval/summarize.py <experiment-dir>/results.jsonl
```

The oracle runs submissions natively on an x86-64 Linux host, under seccomp. Run untrusted
submissions only on a machine you can afford to lose.

## Caveat

The oracles and reference solutions are public. A model trained on this repository may have
seen them, so a new comparison should use new tasks.

[RESULTS.md](RESULTS.md) summarizes the trials run so far. The run directories (submissions,
transcripts) are not part of this repository.
