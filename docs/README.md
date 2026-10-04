# Mukoz specification and design

**Applies to:** Mukoz 0.1.0 (design revision 0.5).
**Status:** Part of the design is implemented. The "Implementation" and "What was verified"
sections of each chapter, and [§4.10](04-contract-language.md), state what is implemented.
`linux-x86_64` meets the acceptance criteria of [§9.8](09-implementation-plan.md) (see
[§2.6](02-platform-model.md)). Everything else is design: not implemented or measured.

**Goal:** AI agents can generate and repair binaries that satisfy a contract, without source. The
trustworthiness of Mukoz's verdicts is a constraint for that purpose and is not relaxed.

Mukoz is a test tool. It checks under which conditions and in which scope an artifact (a binary,
or a machine-code routine with an explicit entry point) satisfies a written contract, and returns
counterexamples and scope as machine-readable evidence.

## Reading order

1. [01 Concept and the boundary of guarantees](01-concept.md)
2. [02 Platform model](02-platform-model.md)
3. [03 Architecture and data model](03-architecture.md)
4. [04 Contract, Binding, and Suite](04-contract-language.md)
5. [05 Artifact inspection and execution](05-execution.md)
6. [06 Evidence and judgement](06-evidence-and-judgement.md)
7. [07 Interface](07-interface.md)
8. [08 Security](08-security.md)
9. [09 Implementation plan, testing and self-check](09-implementation-plan.md)
10. [10 Design decisions, open issues, and references](10-decisions.md)
11. [11 Usage guide for agents](11-agent-guide.md) — the generate-and-repair loop, how to read REJECT / HOLD, how to write contracts and Suites
12. [12 Process boundary and module split (implementation)](12-process-and-modules.md) — the effect model of system calls, files, link files, boundary monitoring, blame, native-process and Mach-O

Agents writing or fixing binaries should start with chapter 11.

## Notation

- "Shall", "shall not", and "required" state requirements for the implementation.
- `[U]` means unverified. `[R]` means inference; the basis and the conditions under which it fails are given with it.
- "Observation" means a check actually performed, and the count is stated ("once" = observed once, not a general claim).
