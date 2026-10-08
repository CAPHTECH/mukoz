---
layout: post
title: "We asked AI agents to write machine code directly. Here is what a hidden oracle found."
author: CAPH TECH Inc.
---

At CAPH TECH we had Claude agents write x86-64 and AArch64 machine code without a compiler, and
judged every submission against a hidden oracle. These are our own trials, not an evaluation by
the model vendor; model names are the ones recorded for each run. Three results stood out:

- **One model generation changed the picture.** In one-shot runs with no way to test,
  Claude Haiku 4.5 passed 1 of 16 runs (eight small task/ISA combinations, two runs each).
  Claude Haiku 5.5 passed 13 of 16, with the same tasks and the same prompt.
- **Checking mattered where the host could not execute the code natively.** On AArch64, from an x86-64 host,
  Haiku 5.5 passed 2 of 8 one-shot runs. Runs that could check their work passed 8 of 8, both
  with our checker and with simulators the agents wrote themselves.
- **One sentence changed the design.** Asked for speed, all four Sonnet 5.5 runs used SSE2;
  none of the 29 other submissions we examined for SIMD did. Asked for size, the binaries came
  out at a third to a half of the size of the runs with no priority.

The numbers are small: 2 to 8 runs per condition. Read them as observations, not rates. The
full record is in
[eval/RESULTS.md](https://github.com/CAPHTECH/mukoz/blob/main/eval/RESULTS.md). The speed
figures come from a separate micro-benchmark we ran; they are not in that file.

## How we tested

[Mukoz](https://github.com/CAPHTECH/mukoz) checks whether a binary satisfies a written
contract. It needs no source code: it runs the machine code itself, in an emulator or under a
sandbox, and reports for each property whether it held, with counterexamples, or that it could
not be checked.

For each task we wrote a contract (inputs, results, permitted memory and effects), a binding
(which registers and memory the contract refers to) and a suite of cases. The tasks were small
routines: memmove, hex and base64 encoding, UTF-8 validation and conversion to UTF-16, and
`codec`, a routine with four converters behind one entry point. In the reach trials we also
used a to-do command-line program, run as a Linux process.

Agents worked under one of three conditions:

| Condition | What the agent could do |
|---|---|
| `A` | Run Mukoz on its binary a limited number of times (20–40 in the larger trials) and read the counterexamples |
| `B` | Test its binary any way it liked. On x86-64 that meant running it; on AArch64 it had to write its own simulator |
| `Z` | No execution or simulation. Write the binary in one go (Python could still be used to lay out the bytes) |

In most trials the agents were told not to use an assembler or a disassembler, so they encoded
instructions by hand and wrote the bytes with Python. After each submission, a hidden oracle
that shares no code with Mukoz judged it: routines on up to 3000 random cases, the to-do
program on 300 random command sessions.

## Haiku 4.5 to Haiku 5.5

We repeated a 16-run calibration from earlier this month: six tasks on x86-64 and two on
AArch64, two runs each, all one-shot (`Z`).

| | Haiku 4.5 | Haiku 5.5 |
|---|---|---|
| x86-64 (12 runs) | 1 pass | 12 pass |
| AArch64 (4 runs) | 0 pass | 1 pass |
| Total | 1/16 | 13/16 |

Most of Haiku 4.5's failures on these routines were instruction-encoding mistakes, and in the
larger to-do trials all 18 of its attempts stopped at stubs. Haiku 5.5's failures were all on
AArch64: one rejected valid input, one read outside its permitted memory, and one run ended
without a submission.

In the runs we looked at, the agents' self-reported confidence did not separate passes from
failures: the two failing submissions came with 20% and 5%, and passing ones with 10% and 30%.

## Checking, on an ISA the host cannot execute natively

Haiku 5.5 sits in the band we had not been able to reach before: it can write these programs,
but not always correctly. So we ran it on the two AArch64 tasks, four runs per task and
condition:

| Condition | utf8_to_utf16 | codec | Total |
|---|---|---|---|
| `A`: Mukoz | 4/4 | 4/4 | 8/8 |
| `B`: own simulator | 4/4 | 4/4 | 8/8 |
| `Z`: one shot | 1/4 | 1/4 | 2/8 |

In three of the eight `A` runs the first check was `REJECT`, and a later one accepted the
changed binary. By the agents' reports, the fixes were a wrong range constant and a
mis-encoded immediate, a return address clobbered by `bl` together with a one-byte overrun,
and an inverted length check.

The `B` agents did just as well. Each wrote an AArch64 decoder and interpreter in Python, and
several wrote fault-injection scripts to check that their own tests could catch mistakes. In
their reports, each also noted the weakness of that approach: the encoder and the simulator were
written from the same reading of the architecture manual, so a misreading common to both would
pass. All eight `B` submissions passed the hidden oracle; that does not show that no shared
misreading exists.

So what we observed is that runs with a way to check did better than one-shot runs, not that
our checker beat the alternative. What a shared tool could save is the simulator each agent
would otherwise write; we did not measure that cost.

## What directly generated binaries look like

We disassembled 21 submissions from the token-cost comparison and an Opus 5.5 calibration: 4
built from C by gcc and 17 written without a compiler.

- **No calls.** gcc `-O2` inlined every helper in the C versions: the codec sources had six
  static functions and each binary had one function with no `call`. The 17 routines written
  without a compiler had no `call` either. Some repeated the same instruction pattern instead:
  one checked a UTF-8 continuation byte with the same six instructions six times.
- **The ABI, by avoidance.** The C binaries used two to five callee-saved registers and saved
  each one. Fifteen of the 17 others used no callee-saved register at all.
- **Tables, not arithmetic.** All five codec routines written without a compiler kept a 64-byte
  and a 256-byte base64 table. Both C versions computed the characters with comparisons. That
  was a choice made in the C source, not by the compiler, and it mattered: on 64 KiB of random
  input, the C base64 encoders ran at 11.6–11.8 TSC ticks per byte against 0.68–0.88 for the
  table versions. On 4 KiB inputs the gap shrank to 1.26–1.31 against 0.70–0.88. Our guess, not
  measured, is that the benchmark repeats the same input and the branch predictor learned it.
- **Habits from the agent's own tools.** One agent's jump helper always emitted 32-bit
  displacements, so every branch in its codec was the long form. Agents writing hex by hand
  used short jumps almost everywhere, and one produced the smallest binary of all: 175 bytes,
  against 655–668 from gcc.

## Asking for speed or size

Sonnet 5.5 wrote hand-encoded x86-64 utf8_to_utf16 and codec, two runs each with no priority,
"as fast as you can", or "as small as you can", correctness first in every case. All 12 passed.

| | utf8_to_utf16 | codec |
|---|---|---|
| No priority | 394–418 bytes | 1551–1675 bytes |
| Speed | 492–517 bytes | 1848–1994 bytes |
| Size | 168–194 bytes | 596–676 bytes |

The speed runs used SSE2 to convert runs of ASCII 8 or 16 bytes at a time. We timed them on
one machine (an AMD Ryzen 9 9955HX), on 64 KiB of ASCII. The utf8_to_utf16 speed runs took 0.06
TSC ticks per byte, against 0.66–0.73 for the runs with no priority and 0.96 for a C version
from the token-cost comparison built with gcc `-O2`. In codec the same conversion took 0.06–0.13
against 0.53–1.07. On inputs with many multi-byte characters the speed runs were usually slower
than the runs with no priority. The size runs used `lodsb`, `stosw` and short jumps, computed
the base64 alphabet instead of storing it, and were the slowest.

One sentence in the request was enough to change the design: SIMD where it pays off on ASCII,
string instructions and a computed base64 alphabet where bytes count.

## What it cost in tokens

We counted task tokens: the agent's context at the end minus its context at its first model
call. With Opus 5.5 and two runs per condition, utf8_to_utf16 on x86-64 took a mean of 8.4k
task tokens in C, 11.0k in assembly and 20.7k hand-encoded. Writing hex with no script at all
was not cheaper (59.0k and 23.4k). The transcripts do not contain the thinking text, so we
estimated its share from what remained: 74–96%. Everything the agents wrote into tool calls,
including the source or hex, commands and final report, came to 1–4k tokens (9k in one
hand-encoded codec run). If that estimate holds, a denser way of writing bytes would not save
much.

## What went wrong

- **Our setup.** The first eight `A` runs of the AArch64 trial started before the emulator was
  installed next to the `mukoz` binary. The four utf8_to_utf16 runs got only `HOLD`
  (`EMULATOR_UNAVAILABLE`), not a pass. Of the four codec runs, three ran their checks after we
  installed it and one ran none. We discarded all eight and ran new ones.
- **Refusals.** In the hand-encoded codec runs on x86-64, a safety classifier stopped responses
  in all four runs we have transcripts for. Three continued on another model; one ended without
  a submission. No other transcript we checked showed a refusal, and we do not know what
  triggers it.
- **Rules.** One agent left its generator script outside its working directory. Another
  reported that it had run another run's generator from the parent directory; its own
  submission used a different design.

## Limits

- Small routines and one small program, two to eight runs per condition.
- Speed was measured on one machine, and Mukoz does not check speed.
- We did not study regenerating software over time as a way to maintain it.
- A verdict is only as strong as the suite. In one earlier trial, a mutation was exposed by 1 of
  1570 generated cases.

In the final verdicts we recorded across these trials, Mukoz did not accept any submission
that the oracle failed. That is an observation, not a guarantee.

## Try it

Mukoz is open source, under the MIT or Apache 2.0 license at your option (the optional `emu/`
backend is GPL):
[github.com/CAPHTECH/mukoz](https://github.com/CAPHTECH/mukoz). The task contracts are in
[eval/tasks](https://github.com/CAPHTECH/mukoz/tree/main/eval/tasks), and
[docs/11](https://github.com/CAPHTECH/mukoz/blob/main/docs/11-agent-guide.md) is the guide we
give to agents.
