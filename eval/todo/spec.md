# Task: todo (a command-line to-do list, Linux x86-64 process)

Write a complete program as **raw x86-64 machine code** for Linux. The file is loaded at address 0x100000 (read + execute only) and execution starts at its **first byte** as the process entry point (`_start`).

## Process environment

- At entry, `rsp` points to `argc`, followed by `argv[0..argc)` pointers, a NULL, an empty environment (NULL) and an empty auxiliary vector. `rsp` is 16-byte aligned. All other general registers are zero. `argv[0]` is `"todo"`.
- Writable memory: the stack (at least 64 KiB below the initial `rsp`) and a zero-filled data area of 65536 bytes at address 0x10000000. The code itself is not writable.
- The working directory contains (or not) the data file `todo.db`. Other paths do not exist.
- Only these system calls are available (Linux x86-64 numbers): `read` (0), `write` (1), `open` (2), `close` (3), `lseek` (8), `openat` (257), `exit` (60), `exit_group` (231). Any other system call ends the run as a failure.
- The program must end with `exit` or `exit_group`. Returning from the entry point is a failure.

## Data file `todo.db`

A sequence of 72-byte records, no header. Record layout:

| bytes | content |
|---|---|
| 0..4 | id, unsigned 32-bit little-endian, ≥ 1 |
| 4 | done flag: 0 or 1 |
| 5 | text length L, 1..64 |
| 6..8 | zero |
| 8..72 | the text in the first L bytes, zero bytes after it |

Records are in strictly increasing id order. A missing file means an empty list. At most 50 records. You may assume an existing file is well-formed.

## Commands

`argv[1]` selects the command. A command with the wrong number of arguments counts as unknown.

| command | effect | stdout | exit |
|---|---|---|---|
| `add TEXT` | append a record: id = (id of the last record) + 1, or 1 if the list is empty; done = 0. Create the file if missing. | `added <id>\n` | 0 |
| `list` | nothing changes (a missing file stays missing) | one line per record in file order: `<id> [ ] <text>\n` or `<id> [x] <text>\n` (x when done); `no items\n` if there are no records | 0 |
| `done ID` | set the done flag of the record with that id to 1 (also if already 1) | `done <id>\n` | 0 |
| `rm ID` | remove the record with that id | `removed <id>\n` | 0 |
| `clear` | remove every record whose done flag is 1 (a missing file stays missing) | `cleared <count removed>\n` | 0 |

Numbers in output are unsigned decimal without leading zeros. `TEXT` and `ID` are `argv[2]`.

Errors (nothing is written to stdout, the file is left exactly as it was, including whether it exists):

| situation | stderr | exit |
|---|---|---|
| unknown command, wrong number of arguments, or no arguments | the usage message (below) | 2 |
| `add` with TEXT empty or longer than 64 bytes | `error: bad text\n` | 1 |
| `add` when the list already has 50 records (checked after the text) | `error: full\n` | 1 |
| `done`/`rm` with an ID that is not 1 to 10 ASCII digits or whose value exceeds 4294967295 (leading zeros are allowed) | `error: bad id\n` | 1 |
| `done`/`rm` with a valid ID that no record has | `error: no such item\n` | 1 |


The usage message is exactly this line followed by a newline (0x0a):

```
usage: todo add TEXT | list | done ID | rm ID | clear
```

After a successful `add`, `done`, `rm` or `clear` that changes the list, the file must contain exactly the new list in the format above. `done`, `rm` and `clear` on a missing file behave as on an empty list (`clear` prints `cleared 0`); they must not create it. TEXT bytes are printable ASCII (0x20..0x7e).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
