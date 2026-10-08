# 01 Concept and the boundary of guarantees

## 1.1 What Mukoz does

```text
artifact + contract + Binding + environment model + execution policy
                    │
                    ▼
                  Mukoz
                    │
                    ▼
  per-property results / counterexamples / scope / what could not be checked
```

- **Artifact**: a whole executable file, or a machine-code routine with an explicit entry point. Source code and debug information are optional auxiliary information.
- How the artifact was made (compiler, direct AI generation, hand-written assembly) does not matter. The basis of a verdict is the contract and observation, not the generator's explanation.
- Mukoz does **unit tests on binaries**. The granularity may be the same as a unit test; the difference is that it checks "the final artifact itself".

The boundary of a unit is defined by the following tuple.

```text
Unit = entry point + initial state + calling convention + permitted effects + completion condition
```

Even when function names are gone, this boundary can be specified per artifact in a Binding. Automatically recovering boundaries from an optimized binary is not a goal.

## 1.2 Principles

| ID | Principle |
|---|---|
| P1 | The correct answer is determined from the contract. Do not work backwards to the expected value from the artifact's output or the generator's explanation. |
| P2 | Separate the contract (what is correct) from the Binding (which registers and memory it corresponds to). The contract does not depend on the ISA. |
| P3 | What could not be checked (unsupported, unobservable, over budget, missing measurement) is not turned into success. |
| P4 | Store results tied to the subject, contract, Binding, environment model, execution platform, and checker version. Do not reuse them for another subject. |
| P5 | Do not silently pass unsupported instructions, syscalls, or formats through to the host, or replace them with a NOP or a success response. |
| P6 | Treat the execution host and the target platform independently. Do not put concepts that assume a specific host or ISA at the core. |
| P7 | Keep the pass/fail of a test and the permission to release or deploy as separate pieces of information. Mukoz does not emit the latter. |
| P8 | Do not equate the generator with the judge. Do not make the AI's self-report a condition of admission. |

## 1.3 Kinds of claims

| Claim | Example | Basis and limits |
|---|---|---|
| Structure | The ELF program headers are consistent | Parser and structural rules. Not functional correctness. |
| Finite behavior | The addition contract was satisfied in 4,160 cases | Limited to the cases run, the environment model, and the execution platform. |

Claims about all inputs (bounded model checking, refinement) are outside the scope of this document set; only room for extension in the data model is kept ([10](10-decisions.md)).

Distinguish the following when writing.

- Being able to disassemble is not evidence of correctness.
- Success in many cases is not correctness on all inputs.
- Success on an emulator does not show that the real OS loader accepts the artifact.
- Success on real hardware does not show that there were no unobserved effects (file writes, communication).

## 1.4 Points to watch in contracts

- **Do not add numeric meaning.** A 64-bit wrapping addition generally does not satisfy a contract that requires mathematical integer addition. Either write the wrapping into the contract, restrict the input range, or write the result on overflow.
- **Do not fabricate behavior on precondition violation.** "Reject if n ≤ 0" cannot be derived from `requires n > 0`. To check the response to invalid input, write that contract.
- **Separate the frame from the write prohibition.** "Restored on return" and "never written during execution" are different properties.
- **Do not call a Binding a proof.** That a Binding points inside the artifact's range and that it is a semantically correct correspondence are different things. The latter remains an assumption.

## 1.5 Out of scope

The following are out of scope. Only boundaries for extension are provided.

- Reproducing arbitrary GUI applications, the whole OS, dynamic linkers, and language runtimes
- Kernels, drivers, and exhaustive exploration of multithreading
- JIT and self-modifying code, side channels, and proofs of termination
- Use as a general-purpose malware sandbox
- Formal verification (proofs over all inputs) and integration with higher-level specification languages

Do not equate formats the parser can read with formats that can be executed and checked.

## 1.6 Terms

| Term | Meaning |
|---|---|
| Artifact | An immutable snapshot of the artifact to check. Identified by digest. |
| Contract | Inputs, state, preconditions, postconditions, permitted effects, and completion condition. ISA-independent. |
| Binding | The correspondence between the contract's variables and the artifact's entry point, registers, memory, and effects. Platform-dependent. |
| Suite | A set of how inputs are generated, the number of cases, the budget, and the required claims. |
| Target platform | The (ISA, format, ABI, OS) tuple on the subject side. [02](02-platform-model.md) |
| Host platform | The (OS, CPU) and versions of the host on which Mukoz runs. |
| Executor | The way the subject is run. emulated / native / translated. |
| Environment model | A model of the OS and external behavior seen from the subject (syscall responses, fault injection). |
| Plan | Everything above fixed and expanded into a case set. Immutable. |
| Run | One execution of a Plan, and its observations. |
| Claim | The result for one property, and its scope. |
| Counterexample | The concrete input, initial state, and environment responses needed to reproduce a failure. |
| Assessment | The admission (ACCEPT_WITHIN_SCOPE / HOLD / REJECT) against the current context. |

## 1.7 Why the contract, not the binary

The trials in [eval/RESULTS.md](../eval/RESULTS.md) inform why Mukoz checks artifacts against contracts. Each point below gives the observation with its scope, then our reading [R].

- **The artifact may change; the contract stays fixed.** One contract judged utf8_to_utf16 binaries that Opus 5.5 produced through C, assembly, hand encoding and direct hex (x86-64, two runs each). Asking Sonnet 5.5 for speed made all 4 such runs use SIMD (x86-64 utf8_to_utf16 and codec, two runs each), which none of the other 29 submissions examined for SIMD did. [R] What the artifact looks like depends on how and under what request it was made, so a result is tied to the artifact's digest together with the contract, Binding, environment, platform and checker version (P4), and its meaning comes from the contract (P1).
- **A generated artifact needs its own check.** On AArch64 (utf8_to_utf16 and codec, four runs each), Haiku 5.5 passed 2 of 8 one-shot runs; runs that could check their work passed 8 of 8 with Mukoz and 8 of 8 with their own simulator. [R] A passing check does not vouch for another artifact from the same generator, and the generator's account of its output is not independent evidence (P8).
- **Effects can outlast the code.** [R] Data written, files changed and messages sent can remain after a program that runs once is gone. That is why a contract lists the permitted effects and why an effect that could not be observed is reported, not assumed absent (P3).
- **A fix becomes part of the program.** [R] A generated patch lives on in the program it fixes, so the old counterexample should stay fixed and the contract should keep holding. Counterexamples are kept as regression cases tied to the contract digest and target, not to one artifact (ADR-18 in [10](10-decisions.md)).
- **The design is carried outside the code.** The 17 x86-64 routines in the trials written without a compiler had no `call`. [R] When code is regenerated rather than maintained, the contract, the Binding and the suite are what carry the design from one artifact to the next, so they are what to version and review.

Not established: whether this makes generated software a different kind of thing, or only makes regenerating it cheaper. The trials included check-and-fix iterations, but did not measure repeated regeneration as a way to maintain software, or its cost.
