# Task: todo, built from modules (a command-line to-do list, Linux x86-64 process)

Write the program as **8 raw x86-64 machine-code files**: a main program and 7 routines it calls (section "Module structure" at the end). The files are linked as described there; execution starts at the **first byte of `main.bin`**, which is loaded at address 0x100000, as the process entry point (`_start`).

## Process environment

- At entry, `rsp` points to `argc`, followed by `argv[0..argc)` pointers, a NULL, an empty environment (NULL) and an empty auxiliary vector. `rsp` is 16-byte aligned. All other general registers are zero. `argv[0]` is `"todo"`.
- Writable memory: the stack (at least 64 KiB below the initial `rsp`) and a zero-filled data area of 65536 bytes at address 0x10000000. The code itself (all modules) is not writable.
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


## Module structure

Eight files, each raw x86-64 machine code whose first byte is its entry point:

| file | loaded at | role |
|---|---|---|
| `main.bin` | 0x100000 | the process entry point; implements the commands above using the routines |
| `parse_id.bin` | 0x200000 | routine (import slot 0) |
| `udec.bin` | 0x300000 | routine (import slot 1) |
| `find_rec.bin` | 0x400000 | routine (import slot 2) |
| `fmt_line.bin` | 0x500000 | routine (import slot 3) |
| `make_rec.bin` | 0x600000 | routine (import slot 4) |
| `del_rec.bin` | 0x700000 | routine (import slot 5) |
| `clear_done.bin` | 0x800000 | routine (import slot 6) |

**Import table.** The 8-byte little-endian address of the routine in slot k is stored at address `0xf0000 + 8*k` (read-only). Code calls a routine through it, e.g. `call qword ptr [0xf0000 + 8*k]` (encoding `ff 14 25` followed by the 32-bit address). There are no other relocations.

**Routines** follow the System V x86-64 calling convention: arguments in rdi, rsi, rdx, rcx; result in rax; they must preserve rbx, rbp, r12–r15 and rsp, and return with `ret`. They may use the stack below rsp but must not write any other memory than stated. **Routines must be position-independent** (address their own constant data RIP-relative): each routine is also tested on its own at another address. A routine may call other routines through the import table (`fmt_line` may use `udec`).

Records below are the 72-byte records of `todo.db`. "all ones" is 0xFFFFFFFFFFFFFFFF.

| routine | arguments | effect and result |
|---|---|---|
| `parse_id` | rdi = pointer to bytes, rsi = length (0..16) | rax = the value if the bytes are a valid ID (1 to 10 ASCII digits, value ≤ 4294967295, leading zeros allowed), otherwise all ones. Reads only those bytes. |
| `udec` | edi = value (32-bit), rsi = output pointer | writes the unsigned decimal text of the value (no leading zeros, 1..10 bytes) to the output; rax = its length. Writes nothing else. |
| `find_rec` | rdi = pointer to n records, rsi = n (0..50), edx = id | rax = index (0-based) of the record with that id, or all ones if none. Reads only the records. |
| `fmt_line` | rdi = pointer to one record, rsi = output pointer | writes the list line of that record, `<id> [ ] <text>\n` or `<id> [x] <text>\n`, to the output; rax = its length (at most 80). Writes nothing else. |
| `make_rec` | rdi = destination (72 bytes), esi = id, rdx = text pointer, rcx = text length (1..64) | writes a complete record: that id, done 0, length, zero bytes, the text, zero padding. |
| `del_rec` | rdi = pointer to n records, rsi = n (1..50), rdx = index (< n) | removes the record at that index: the later records move down one slot; rax = n − 1. The last slot's bytes may be left as anything. |
| `clear_done` | rdi = pointer to n records, rsi = n (0..50) | moves the records with done flag 0, in order, to the front; rax = their count. The bytes after them may be left as anything. |

Deliverable: the 8 files above in the working directory.
