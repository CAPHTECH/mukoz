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

## What the submissions look like

After the trials, we disassembled the submissions to see which mistakes Mukoz had to catch and
how the binaries were built. Classifying a mistake, and naming the instruction the agent
intended, is our reading of the disassembly, the evidence stores and the spec; the agents did
not report it. AArch64 words were decoded by hand (no disassembler on the host).

**Mistakes in failing final submissions.** In the routine trials, 29 final submissions failed
the oracle: 28 from Haiku 4.5 (exp3 and exp4, 36 submissions) and 1 from Opus 5.5 (70 judged
submissions: 39 from the first trial and the counterexample trial, 16 from the Opus calibration
and 15 from the token-cost comparison). The 9 Sonnet 5.5 to-do submissions of exp5 all passed.
Several Haiku submissions had more than one problem; the table gives the one we judged to be
the main cause of each failure, so the rows are our classification, not a measured order:

| Main cause (our classification) | Haiku failures |
|---|---|
| Instruction encoding: REX.R and REX.B swapped, a wrong SIB index, a reversed ModRM direction, an unresolved jump displacement, a wrong A64 `RET` or `B` encoding, an out-of-range `STRB` offset | 13 |
| A signed comparison against 0x80 (`cmp` then `jl`/`jge`), so ASCII bytes were taken as lead bytes | 4 |
| Unfinished: the code handled only part of the task (for example, it wrote one fixed unit and returned 1) | 4 |
| AArch64 return value: `x0` still held the `dst` pointer | 3 |
| Spec or logic: a leading continuation byte accepted, memmove's overlap direction reversed | 2 |
| ABI: `sp` was 0 at `ret` | 1 |
| Not classified: a store through a register that was never set, which may be an encoding or a register mistake | 1 |

- Three more Haiku failures also changed a callee-saved register (x28, r12, rbx) without
  restoring it; we counted another problem as their main cause.
- The one Opus failure (utf8_to_utf16 on AArch64) accepted the surrogate `ED A0 80` and
  rejected valid input just below it (`ED 9F BF`): its surrogate range check, built on
  subtracting 0xD000, was misplaced. Whether that came from misreading the spec or from a slip
  is not known.
- No other Opus or Sonnet final submission failed. The models were given different task sets
  (only utf8_to_utf16 was common to all three), so this does not show that Opus and Sonnet
  never make encoding mistakes. The Sonnet calibration (exp2) and the Haiku to-do runs were not
  disassembled.

**Mistakes on the way, in condition A.** The evidence stores of the condition-A runs keep the
checks that were stored:

| Model | Runs | Stored checks | `REJECT` | Notes |
|---|---|---|---|---|
| Opus 5.5 | 20 | 21 | 1 | The only `REJECT` was the bug injected into a fix task |
| Sonnet 5.5 | 3 | 6 | 2 | One run: the miscounted argc, fixed from the counterexample |
| Haiku 4.5 | 18 | 338 | 309 (and 18 `HOLD`) | 3 runs reached `ACCEPT_WITHIN_SCOPE` for the whole routine (memmove), 2 of them after 2 and 23 `REJECT`s; one more run had only individual modules accepted |

Of the 327 Haiku checks that did not accept, these properties were violated (one check can
violate several): memory access outside the permitted regions 146, a functional `ensures` 142,
a forbidden or faulting instruction 33, termination (returned or exited) 29, a callee-saved
register 5. 259 of the 327 involved an out-of-range access or a wrong result.

**Structure of the binaries.** Compared on the token-cost comparison (13 x86-64 submissions)
and the Opus calibration (8 x86-64 submissions): 4 compiled from C and 17 written without a
compiler (assembly, hand-encoded or direct hex).

- **Functions.** In the C condition, gcc -O2 inlined every helper: the codec sources had 6
  static functions, and each binary had one function with no `call`. The 17 routines written
  without a compiler had no `call` either; some repeated an instruction pattern instead (one
  hand-encoded utf8_to_utf16 has the same six-instruction continuation-byte check six times,
  with different displacements and immediates). In the to-do processes, the hand-encoded
  submissions did split the work: 4–7 call targets in exp5 (9 runs), 13–15 in exp6 (3 runs).
- **Callee-saved registers.** The 4 C binaries used 2–5 callee-saved registers and saved every
  one. 15 of the 17 routines written without a compiler used none; the other 2 saved `rbx`.
  Inside the hand-encoded processes, internal functions mostly saved nothing and shared the
  callee-saved registers like globals. That is legal inside one process; once the pieces are
  checked separately, boundary monitors ([docs/12](../docs/12-process-and-modules.md)) check
  the ABI's saving rules at each boundary.
- **Data.** All 5 codec routines written without a compiler kept a 64-byte and a 256-byte
  base64 table after the code. The 2 C codecs computed the characters with comparisons and had
  4 bytes of read-only data. This is a choice in the source, not an effect of the compiler.
- **Encoding habits.** The 3 hand-encoded codec submissions used 32-bit displacements for all
  of their 76–88 branches (6-byte conditional and 5-byte unconditional jumps); in one of them we
  checked that the agent's own jump helper always emitted that form. The 2 direct-hex
  submissions used short jumps for 16 of 17 and 20 of 22 branches. gcc padded with 29–259 bytes
  of `nop` for alignment; the 17 others had none.
- None of these 21 submissions used SIMD instructions.

## Later trials: Haiku 5.5

These ran after the summary above, on Claude Haiku 5.5 (`claude-haiku-5-5` in the agent
transcripts) and Sonnet 5.5, with the same task files, prompts and hidden oracle as before.

**One-shot calibration repeated.** The 16 one-shot runs of the Haiku 4.5 calibration
(memmove, hex_encode, utf8_count, base64, utf8_to_utf16 and codec on x86-64; utf8_to_utf16 and
codec on AArch64; two runs each), with the same prompt:

| | Haiku 4.5 | Haiku 5.5 |
|---|---|---|
| x86-64 (12 runs) | 1 pass (hex_encode) | 12 pass |
| AArch64 (4 runs) | 0 pass | 1 pass: one utf8_to_utf16 rejected valid input, one codec read outside its regions, one codec run has no submission (its agent's report: it could not check its work) |
| Total | 1/16 | 13/16 |

Mukoz agreed with the oracle on all 15 submissions.

**AArch64 with and without a check.** Haiku 5.5 on utf8_to_utf16 and codec for AArch64, four
runs per task and condition, with the prompts of the earlier Haiku trials:

| Condition | utf8_to_utf16 | codec | Total |
|---|---|---|---|
| `A`: Mukoz | 4/4 | 4/4 | 8/8 |
| `B`: own tests (every agent wrote its own A64 simulator) | 4/4 | 4/4 | 8/8 |
| `Z`: one shot | 1/4 | 1/4 (one without a submission) | 2/8 |

- In 3 of the 8 `A` runs the first stored check was `REJECT` and a later one accepted the
  changed binary. By the agents' reports, the fixes were a wrong range constant and a
  mis-encoded immediate, a return address clobbered by `bl` together with a one-byte overrun,
  and an inverted length check. The other 5 were accepted on the first check.
- The agents' final reports (not kept in this repository) say more about `B`: each noted that
  its encoder and its simulator shared one reading of the encoding, so a misreading common to
  both would not be caught, and some spent much of their effort on bugs in their own simulator.
  All 8 `B` submissions passed the oracle; that does not show that no shared misreading exists.
- Of the 30 submissions judged (the discarded runs below included), 23 were accepted and
  passed, 6 were rejected and failed, and 1 was `HOLD` (an instruction the emulator does not
  support) and failed. Mukoz accepted none that failed.
- A setup mistake on our side: the first `A` runs started before the emulator was installed next
  to the `mukoz` binary. The four utf8_to_utf16 runs got only `HOLD` (`EMULATOR_UNAVAILABLE`);
  of the four codec runs, three ran their checks after we had installed it and one ran none. We
  discarded all eight and ran four new ones per task; the table shows only the new ones.
- One codec agent left its generator outside its working directory (`gen/` beside the run
  directories). In the priority trial below, one agent reported that it had run another run's
  generator from the parent directory; its own submission uses a different design.

**Asking for speed or size.** Sonnet 5.5, hand-encoded x86-64 utf8_to_utf16 and codec, one
shot, two runs each with no priority, "as fast as you can" or "as small as you can" (both after
correctness). All 12 passed the oracle and Mukoz.

- Size: utf8_to_utf16 was 394–418 bytes with no priority, 492–517 for speed and 168–194 for
  size; codec was 1551–1675, 1848–1994 and 596–676.
- All 4 speed runs used SSE2 for runs of ASCII. None of the other 8 used SIMD, nor did any of
  the 21 submissions described above.
- The size runs used string instructions (`lodsb`, `stosw`) and short jumps, and computed the
  base64 alphabet instead of keeping a table.
- Mukoz does not check speed. We measured it separately: compared with the runs with no
  priority, the speed runs were faster on some inputs and slower on others.

## What we conclude, and what we do not

- [R] **No difference in success rate was observable.** In the reach trials, the tasks never
  fell into the band of "can be written, but with mistakes":
  - todo2 and everything smaller was too small for Sonnet, which wrote it correctly in one go.
  - The to-do program's main was too large for Haiku, which did not finish it.

  Basis: the reach-trial table, 3–4 runs per condition. This may change with Sonnet on tasks of
  8 KB or more, or with Haiku on tasks split into smaller pure routines.
- [R] **With Haiku 5.5 on AArch64, the runs that could check their work did better.** `A` and
  `B` each passed 8 of 8, against 2 of 8 for one-shot runs. This does not show that the two ways
  of checking are equivalent. Basis: 8 runs per condition, two tasks, one model. Not measured:
  whether Mukoz saves the time or tokens of writing a simulator.
- [R] **No counterexample yet against using Mukoz as a judge.** It never accepted a submission
  the oracle failed. However, a verdict is only as strong as the suite: for one to-do mutation, 1 of 1570
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
- [R] **Most of what Mukoz caught in Haiku's routines was below the level of the algorithm.**
  By our classification, encoding mistakes and signed comparisons were the main cause of 17 of
  its 28 failures, and 259 of its 327 non-accepting checks involved an out-of-range access or a
  wrong result. Basis: the disassembly of 28 failures and the stored condition-A checks, all
  Haiku 4.5. Another reader could classify some failures differently.
- [R] **Routines written without a compiler kept the callee-saved rule mostly by not using
  those registers** (15 of 17), and the hand-encoded to-do processes shared registers between
  their own functions. Such private conventions are not visible at a routine's outer boundary;
  they become checkable when the program is split into modules with boundary monitors. Not
  measured here.

The run directories (submissions, transcripts, `results.jsonl`) are not part of this
repository.
