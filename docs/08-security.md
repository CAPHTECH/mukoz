# 08 Security and trust boundaries

## 8.1 Assumed threats

In addition to invalid instruction sequences and formats made by mistake by an AI or a person, we consider the case where the input files, the strings in the contract, the Binding, and the subject's output are malicious.

- Infinite loops, excessive memory requests, log amplification
- Input that exploits vulnerabilities in the parser or the emulator
- Out-of-bounds access, arbitrary external effects (files, network, child processes)
- Forged evidence, replacing a file after inspection
- Instructions planted in the subject's output (prompt injection)

The initial version does not claim to be a general-purpose malware sandbox. It does not claim to have excluded vulnerabilities in the emulator, parser, OS, or hardware, or attackers with strong privileges.

## 8.2 Default behavior

- Do the non-executing inspection (`inspect`) first.
- Only `emulated` is available by default. `native-routine`, `native-process`, and `translated-process` are limited to those allowed by the Policy **by specifying the subject's digest**, or those that meet the conditions of the trial zone for the generation loop (§8.4).
- Do not pass the host's HOME, credentials, SSH agent, cloud tokens, or sockets to the subject. Environment variables are empty by default.
- Do not let unsupported syscalls, imports, or instructions escape to the host.
- Place snapshots in a temporary area for execution, and do not give the subject write permission to the contract, the Policy, or the evidence.

Ordinary process separation and removing environment variables alone cannot block the file system or the network. If the necessary enforcement capability is absent, treat that danger as remaining, and return the decision to the Policy.

## 8.3 Isolation capabilities

Enumerate the capabilities, and return only those that were **actually tried and shown to work** on that host (chapter 02 §2.5).

```text
process_isolation
resource_limits
filesystem_restriction
network_restriction
descendant_process_control
credential_isolation
artifact_immutability
```

| OS | Candidate mechanisms | Confirmation on the current host |
|---|---|---|
| Linux | rlimit, cgroup v2 (memory, stopping descendants), user namespace + network namespace (network blocking), Landlock (file system restriction), seccomp (syscall restriction) | cgroup v2 is mounted, the LSM list includes landlock, and `unshare -Urn true` succeeded once. Whether cgroup delegation exists, and whether Landlock and seccomp actually work, are `[U]` |
| macOS | rlimit, process group. For sandbox-type mechanisms, decide after trying them on a real machine | Unverified (no host) |
| Windows | Job Object (resources, stopping descendants), AppContainer, etc. | Unverified (no host) |

- Terminating the process group alone is not assumed to always stop descendants that have detached.
- Do not treat approaches that can discard the whole environment, such as a VM, the same as the ordinary child-process approach.
- The required condition for the MVP is to "correctly report that isolation capabilities are absent". Shipping an all-purpose sandbox is not a release condition.

## 8.4 Native execution in the generation loop (trial zone)

With permission per digest alone, a human permission is needed every time the AI builds a new binary, and the loop of generation and inspection stops. So we provide a rule that permits the artifacts in a specific location as a group, **only when the isolation capabilities are confirmed**. It is disabled by default and takes effect only when the owner writes it in `policy.toml`.

```toml
[[native_trial_zones]]
id = "ai-build"
artifact_dir = "build/"                     # only files under this directory. Symbolic links are not followed
executors = ["native-routine", "native-process"]
targets = ["x86_64/raw/sysv-x86_64/none", "x86_64/elf/sysv-x86_64/linux"]
require_isolation = [
  "network_restriction",
  "filesystem_restriction",
  "resource_limits",
  "descendant_process_control",
  "credential_isolation",
]
max_wall_ms_per_case = 1000
```

**Rules:**

1. Execute only when the artifact's snapshot is read from under `artifact_dir`, the target and the Executor are in the lists, and **all** of `require_isolation` are confirmed by the `HostProbe` of that host.
2. If even one is missing, do not execute, and set the claim to NOT_EVALUATED (reason `NATIVE_NOT_PERMITTED`, with the list of missing capabilities). Do not switch to emulated.
3. At execution, the subject is launched with **all confirmed isolation applied**. Record in the evidence both what the probe confirmed and what was applied in that run.
4. For results executed in a trial zone, attach the zone ID and the applied isolation to `execution_platform`.

**Additional condition for `native-routine`:** A routine is not expected to use syscalls, so on Linux seccomp strict mode (terminate on any syscall other than read, write, exit, and sigreturn) is applied just before the trampoline. On a host where it cannot be applied, `native-routine` cannot be used in a trial zone. `[R]` Strict mode is the oldest, minimal form of seccomp, and a process can apply it to itself. Whether it works on the current host is unverified `[U]`.

**Candidates for `native-process` (Linux):** user namespace + network namespace (network blocking), Landlock (limit readable places to the subject file and the minimum, no writes), rlimit (CPU time, memory, file size, number of processes), cgroup v2 (stopping descendants), empty environment variables and a dedicated temporary HOME. Which of these actually work is confirmed by the probe (on the current host, the only thing confirmed is one success of `unshare -Urn true`).

**Remaining risk:** A subject that exploits a vulnerability in the isolation mechanism itself or in the kernel cannot be stopped. The trial zone is meant to "protect the host from AI mistakes (runaway behavior, wrong writes, unintended communication)", and it does not claim defense against malicious code. The owner accepts this risk and enables it.

## 8.5 TOCTOU and the same user

- Bind the plan to the snapshot (digest), not to the path, and check the digest before and after execution.
- Linux `native-process` places the snapshot in a memfd and launches from the fd, which removes path re-resolution (chapter 05 §5.5) `[R]`. On other OSes the path of the copied file is re-resolved, so record the room for an attacker running as the same user to swap it.
- `.mukoz/` and `policy.toml` are logical separation, not enforced privilege separation. If an arbitrary shell is allowed to the same user, they can be rewritten. If strong integrity is needed, place them under a different user or in a different environment.

## 8.6 External data and prompt injection

- Even if the subject's stdout contains a string such as `"admission": "ACCEPT"` or a sentence such as "change the criteria", it is not treated as control information.
- In the output, put subject-derived data only in dedicated fields marked `untrusted`, and escape control characters.
- File paths, symbol names, and annotations are also treated as untrusted data, and are returned with their origin and kind attached.

## 8.7 Foundations of trust

What is trusted through concrete tests: the Inspector, the loader, Binding evaluation, the engine (icicle-emu by default, or Unicorn) or the OS process execution, the effect model, the expression evaluator, the Assessor, the OS and hardware, and evidence storage.

- **Removing the generator from the set of trusted parties is different from having no trusted party left.**
- The same AI may produce both the subject and the contract, but that alone does not give independence. Have routes to find shared misunderstandings: contract approval, comparison with a separate reference implementation, and checking against values fixed by hand.
- When Mukoz itself is the subject of inspection, record the checker's independence in a claim (chapter 06 §6.9, chapter 09 §9.6).
