# Results of the comparison trials (October 2026)

The question (docs/10, ADR-22): does an AI agent that writes machine code without source reach
a correct binary more reliably with Mukoz than without, and without false acceptances?

## Setup

- **Agents.** Claude subagents. The first trial does not record which model ran. Later trials
  name the model: Sonnet 5.5 or Haiku 4.5.
- **No assembler.** In every condition, the agents were told not to use an assembler or a
  disassembler. They encoded instructions by hand and laid out bytes with Python. Without an
  assembler, the difference between conditions is about catching encoding mistakes, not about
  having a tool. The rule was given in the instructions only, not enforced.
- **Conditions.**
  - `A`: Mukoz only (contract, binding and suite given; 20–40 checks at most in the larger
    trials).
  - `B`: the agent's own tests. Native execution and a launcher were allowed.
  - `Z`: no execution at all; the agent writes the program in one go.
  - `RA` / `RB`: fix a binary with one injected bug, with or without Mukoz.
- **Judging.** A hidden oracle judged each submission after it was handed in. Routines ran on
  2000–3000 random cases against an independent reference. Processes ran 300 random command
  sessions natively (chroot and seccomp), compared against a Python model of the spec. Before
  each task was used, we checked that its reference passed the oracle and that most of its
  mutations failed.

Every count below is from one run per agent and condition. These are observations, not
estimates of a rate.

## First trial: small routines

There was one run per task and condition.

| Kind | Tasks | With Mukoz (A / RA) | Without (B / RB) |
|---|---|---|---|
| Generate, x86-64 | abs_diff, fill, popcount, checked_mul, memmove, hex_encode, shl_var, isqrt | 8/8 pass; one `mukoz check` each, ACCEPT | 8/8 pass |
| Generate, AArch64 on an x86-64 host | memmove, hex_encode, base64 | 3/3 pass; one check each | 3/3 pass; every agent wrote its own A64 simulator |
| Fix, x86-64 | count_byte, memmove, isqrt (one bug each) | 3/3 pass | 3/3 pass |
| Fix, AArch64 | base64 (wrong immediate in the padding check) | 1/1 pass | 1/1 pass |
| Generate, with a trap | count_byte_fast (word-at-a-time; the well-known zero-byte test gives false positives) | 2/2 pass | 2/2 pass |
| Fix compiler output | base64 (gcc -O2, 384 bytes, one mask changed) | 1/1 pass | 1/1 pass |

- **No false acceptances.** Mukoz did not accept any failing submission (0 of 18). In condition
  B, no agent declared a failing submission done (0 of 18).
- Mukoz accepted all 36 final submissions when they were checked afterwards, matching the
  oracle.
- Every agent reached a correct binary on its first attempt. Mukoz never reported `REJECT`, so
  this trial says nothing about fixing from counterexamples.
- Generating code for the other ISA (AArch64 on an x86-64 host):
  - Condition A took 3 tool calls and 25–51 s.
  - Condition B took 4–6 calls and 58–83 s. Every agent wrote an A64 decoder or interpreter to
    test with, and one of those simulators had its own bug.
  - All condition-B agents noted the same limit: their encoder and their decoder could share a
    misreading.

## Fixing from a counterexample

The task was a gcc `-O3 -funroll-all-loops` base64 routine (512 bytes) with one changed byte in
the second unrolled group. The agent received only docs/12, not instructions for the CLI.

- Both conditions passed.
- With Mukoz, this was the first `REJECT` an agent used to fix code. From the property id
  (`char2`) and the counterexample, it narrowed the bug to "only the third character of odd
  groups is wrong". It then found the bug by comparing the masks of the two unrolled groups (its
  own report; two checks).
- The agents reported that the instruction ring did not reach back to the faulty instruction,
  and that subexpression dumps of large expressions were unreadable. `detail.why_false` was
  added in response.

## Writing contracts

Agents received a spec, docs 04/05/07 and a correct reference binary, and wrote the contract,
binding and suite for memmove and hex_encode.

- Both contracts accepted our reference and rejected our mutations.
- Each agent also tried mutations of its own:
  - hex_encode: 14 of 14 were rejected.
  - memmove: of 17, one non-terminating mutation was `HOLD`, and one signed-comparison mutation
    was not detected. That mutation needs a region above 2^63, which Mukoz cannot place.
- The tool defects these agents reported were all fixed, with acceptance tests and fault
  injection. They were:
  - a `requires` that discarded almost every case still gave ACCEPT;
  - invalid generator items were silently ignored;
  - over-long lengths were silently truncated;
  - the boundary product was silently reduced.

## Reach trials: tasks a model might not write correctly in one go

| Trial | Model and task (size of the gcc reference) | A | B | Z |
|---|---|---|---|---|
| calibration | Sonnet 5.5, utf8_to_utf16 and codec (x86-64 and AArch64) | — | — | 8/8 |
| exp3 | Haiku 4.5, utf8_to_utf16 (x86-64 and AArch64) | 0/8 | 0/8 | 0/8 |
| exp4 | Haiku 4.5, memmove (x86-64) | 3/4 | 3/4 | 2/4 |
| exp5 | Sonnet 5.5, to-do CLI as a process (1687 B) | 3/3 | 3/3 | 3/3 |
| exp6 | Sonnet 5.5, todo2: priorities, search, import (3435 B) | — | — | 3/3 |
| exp7 | Haiku 4.5, to-do CLI | 0/3 | 0/3 | 0/3 |
| exp8 | Haiku 4.5, to-do CLI split into 8 modules (main 1367 B + 7 routines) | 0/3 | 0/3 | 0/3 |

- **Mukoz agreed with the oracle in all 63 judgements** (exp3 judged one submission twice). No
  wrong submission was accepted.
- Sonnet 5.5 wrote every program correctly without running it, up to todo2. Its hand-encoded
  submissions were 2969–3619 bytes, and each took 5.5–7 minutes.
- Sonnet's mistakes in conditions A and B were few. In exp5:
  - One A agent miscounted argc and fixed it from a Mukoz counterexample.
  - Two B agents found encoding mistakes with their own tests: a `jcc` opcode and a `cmp`
    immediate.
- Haiku 4.5 did not *mis*write the to-do programs; it did not finish them. All 18 attempts
  stopped at stubs of 9–608 bytes that never reached file I/O. Splitting into modules did not
  help, because main was still not written. Mukoz accepted only a few individual routines
  (A: 2, B: 1, Z: 0, out of 3 attempts each).
- Things that went wrong in running the trials:
  - A Haiku agent wrote outside its working directory (once).
  - An agent started its own subagent, which kept editing after the report (once).
  - An agent ran the launcher without `unshare` and gave up testing (once).
  - The AArch64 oracle (pure Python) hung for an hour on a non-terminating submission. It now
    stops after 20 failures.

Mukoz defects found during these trials were all fixed:

- argc beyond the binding's argv expressions was silently truncated.
- A boundary monitor evaluated the callee's `ensures` even when the call broke the callee's
  `requires`, and blamed the callee.
- Monitors required generation-only inputs that no condition referenced.
- A monitor's `requires` violation was located at the stack pointer instead of the return
  address.

## What we conclude, and what we do not

- [R] **No difference in success rate was observable.** The tasks never fell into the band of
  "can be written, but with mistakes":
  - todo2 and everything smaller was too small for Sonnet, which wrote it correctly in one go.
  - The to-do program's main was too large for Haiku, which did not finish it.

  Basis: the tables above, 3–4 runs per condition. This may change with Sonnet on tasks of
  8 KB or more, or with Haiku on tasks split into smaller pure routines.
- [R] **No counterexample yet against using Mukoz as a judge.** It agreed with the oracle every
  time. However, a verdict is only as strong as the suite: for one to-do mutation, 1 of 1570
  generated cases exposed it.
- [R] **The clearest benefit was an independent check for another ISA.** Without Mukoz, agents
  verified AArch64 code with simulators they wrote themselves, which share their own
  misunderstanding of the encoding.
- [U] Whether Mukoz raises the rate of reaching a correct binary is not established.

The run directories (submissions, transcripts, `results.jsonl`) are not part of this
repository.
