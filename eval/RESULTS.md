# Results of the comparison trials (October 2026)

The question (docs/10, ADR-22): does an AI agent that writes machine code without source reach
a correct binary more reliably with Mukoz than without, and without false acceptances?

## Setup

- **Agents.** Claude subagents. The run records of the first trial, the counterexample trial and
  the contract-writing trial do not name the model; the agent transcripts show Claude Opus 5.5
  for all of them (41 agents). The reach trials name the model: Sonnet 5.5 or Haiku 4.5.
- **No assembler.** In every condition except those of the token-cost comparison, the agents
  were told not to use an assembler or a disassembler. They encoded instructions by hand and
  laid out bytes with Python. Without an assembler, the difference between conditions is about
  catching encoding mistakes, not about having a tool. The rule was given in the instructions
  only, not enforced.
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
  (`char2`) and the counterexample's inputs and outputs, it narrowed the bug to "only the third
  character of odd groups is wrong". It then found the bug by comparing the masks of the two
  unrolled groups (its own report; two checks).
- The agents reported that the instruction ring did not reach back to the faulty instruction,
  and that subexpression dumps of large expressions were unreadable. `detail.why_false` was
  added in response.
- In a rerun with the new field, the agent found the bug by reading the code first. It reported
  that `why_false` pointed to the same place: the odd group, observed 0x35 where 0x37 was
  expected (one run).

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

- **Mukoz agreed with the oracle in all 63 judgements** recorded in the trial log for these
  experiments. The log notes that exp3 judged one submission twice; it did not keep a breakdown
  per trial. No wrong submission was accepted.
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

## Token cost: compiler, assembler or hand encoding

The question: how many more tokens does an agent spend when it produces machine code without a
compiler, and on what? The trials above did not measure this. Every run below is one-shot, as
in condition `Z`: the agent writes the routine without executing or simulating it, and the
hidden oracle (3000 cases; on AArch64 it stops after 20 failures) and Mukoz judge the
submission afterwards.

**Conditions.** Claude Opus 5.5, two runs per task and condition:

| Condition | The agent writes | Turned into bytes by |
|---|---|---|
| C | Freestanding C | gcc `-O2 -fPIE` and ld, entry function first |
| Assembly | GNU assembler source | as and ld |
| Hand-encoded | Instruction bytes, laid out by its own Python script | Its own script. In these runs the scripts joined the bytes and filled in jumps and RIP-relative references: the displacements and, in one run, the conditional-jump opcode from a condition code. In the earlier calibration below, some agents also wrote functions that packed instruction fields |
| Direct hex | Hex text only. It may not write or run any program or script, except the build script it is given | That build script, which only strips comments and whitespace and converts hex pairs to bytes |

In every condition the agent could run its build step, and compiler or assembler errors were
allowed feedback. Nothing could execute or simulate the code. Before the runs, the gcc
references of both tasks passed the oracle through the C and the assembly pipelines, and a
reference with one byte changed failed it.

**Measure.** Task tokens are the agent's context at the end minus its context at its first model
call. This leaves out the fixed part (system prompt, tool definitions and task prompt, about
33,000 tokens) and counts everything the agent added: thinking, tool calls and tool results. The
transcripts do not contain the thinking text. Its share is estimated as what remains after the
visible tool calls, results and text, at 3.3 characters per token.

| Task | C | Assembly | Hand-encoded | Direct hex |
|---|---|---|---|---|
| utf8_to_utf16, x86-64 | 8.5k, 8.4k | 10.3k, 11.7k | 22.5k, 18.9k | 59.0k, 23.4k |
| utf8_to_utf16, AArch64 | — | — | — | 27.1k, 24.2k |
| codec, x86-64 | 14.2k, 18.2k | 22.3k, 25.5k | 72.5k (partly on Opus 4.8, see below); no submission (23.0k) | — |

- All 15 submissions passed the oracle, and Mukoz accepted each of them. One hand-encoded codec
  run produced no submission.
- Thinking was 74–96% of the task tokens (estimate). Everything the agent wrote into tool
  calls, including the source or hex, its commands and its final report, came to 1–4k tokens
  (9k in the hand-encoded codec run that finished).
- On utf8_to_utf16, the assembler removed most of the gap between hand encoding and C: means of
  11.0k for assembly, 20.7k for hand encoding and 8.4k for C. On codec, assembly still took
  about 1.5 times as many tokens as C.
- Writing hex without any script was not cheaper than hand encoding with a script: 59.0k and
  23.4k. The 59.0k run, by its own report, spent its effort fitting branches into the 8-bit
  displacement range. Its binary was the smallest of all runs: 175 bytes, against 655–668 bytes
  from gcc.
- Direct hex worked on AArch64 too: 2 of 2 passed. There is no C or assembly comparison for
  AArch64, because the host has no AArch64 toolchain.

**Refusals.** Both hand-encoded codec runs were stopped by a safety classifier.

- In one run, Opus 5.5 ended two responses with `stop_reason: refusal` (category `cyber`). The
  remaining 12 of its 15 model calls ran on Claude Opus 4.8. It finished after 26 minutes and
  72.5k task tokens, and passed.
- In the other run, the harness reported that a response had been stopped by a safety
  classifier while the agent was writing its build script; no category was recorded. The agent
  then stopped without a submission, after 23.0k task tokens.
- Both hand-encoded codec x86-64 runs of the earlier Opus calibration (below) were refused with
  category `cyber` and continued on Opus 4.8.
- No other agent transcript shows a refusal. We checked the 115 transcripts of both sessions,
  including the C and assembly runs of the same task and the hand-encoded AArch64 codec runs.
  Runs started from workflows, among them the Sonnet calibration and some of the Haiku trials,
  left no transcripts and were not checked. What triggers the refusal is not known.

**Earlier Opus 5.5 calibration.** Before the Sonnet calibration in the reach trials, 16
hand-encoded one-shot runs ran on Opus 5.5: utf8_count, utf8_to_utf16, utf8_to_utf16_fast and
codec, on x86-64 and AArch64, two runs each. They were not reported until now. Judged again for
this summary:

- 15 of 16 passed the oracle. The failing one (utf8_to_utf16 on AArch64) was also the only
  `REJECT` from Mukoz, so Mukoz agreed with the oracle on all 16.
- Task tokens were 11.2k–20.0k for the UTF-8 tasks, and 22.9k and 27.1k for codec on AArch64.
  Codec on x86-64 took 75.3k and 87.3k; both runs were refused once and continued on Opus 4.8.
- The same prompt on the same model cost more in this comparison. Hand-encoded utf8_to_utf16 on
  x86-64 took 12.7k and 11.5k task tokens in the earlier session, and 22.5k and 18.9k here. The
  cause was not identified; the effort setting and the Claude Code version may differ between
  the sessions. Compare token counts only within one session.

## What we conclude, and what we do not

- [R] **No difference in success rate was observable.** In the reach trials, the tasks never
  fell into the band of "can be written, but with mistakes":
  - todo2 and everything smaller was too small for Sonnet, which wrote it correctly in one go.
  - The to-do program's main was too large for Haiku, which did not finish it.

  Basis: the reach-trial table, 3–4 runs per condition. This may change with Sonnet on tasks of
  8 KB or more, or with Haiku on tasks split into smaller pure routines.
- [R] **No counterexample yet against using Mukoz as a judge.** It agreed with the oracle every
  time. However, a verdict is only as strong as the suite: for one to-do mutation, 1 of 1570
  generated cases exposed it.
- [R] **The clearest benefit was an independent check for another ISA.** Without Mukoz, agents
  verified AArch64 code with simulators they wrote themselves, which could share their own
  misunderstanding of the encoding.
- [U] Whether Mukoz raises the rate of reaching a correct binary is not established.
- [R] **Most of the task tokens went to thinking, not to the text the agent wrote.** Thinking was
  an estimated 74–96% of the task tokens, and the tool calls, source or hex included, were
  1–4k tokens. If that estimate holds, a denser representation of the bytes cannot save much.
  Basis: the token-cost comparison, 2 runs per condition, on Opus 5.5; one hand-encoded codec
  run continued on Opus 4.8.
- [R] **In that comparison, assembly took about half the task tokens of hand encoding.** On
  utf8_to_utf16 (x86-64) the means were 11.0k for assembly, 20.7k for hand encoding and 8.4k
  for C. None of the four C and assembly codec runs was refused; both hand-encoded codec runs
  were. Writing hex directly also gave working binaries, but on utf8_to_utf16 it cost the most
  of the four conditions. Mukoz checks the result either way.
- [U] Not measured: effort settings, a fixed and tested encoder in place of an assembler, and
  fix loops with Mukoz.

The run directories (submissions, transcripts, `results.jsonl`) are not part of this
repository.
