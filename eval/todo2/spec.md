# Task: todo2 (a command-line to-do list with priorities, Linux x86-64 process)

Write a complete program as **raw x86-64 machine code** for Linux. The file is loaded at address 0x100000 (read + execute only) and execution starts at its **first byte** as the process entry point (`_start`).

## Process environment

- At entry, `rsp` points to `argc`, followed by `argv[0..argc)` pointers, a NULL, an empty environment (NULL) and an empty auxiliary vector. `rsp` is 16-byte aligned. All other general registers are zero. `argv[0]` is `"todo"`.
- Writable memory: the stack (at least 64 KiB below the initial `rsp`) and a zero-filled data area of 65536 bytes at address 0x10000000. The code itself is not writable.
- The working directory contains (or not) the data file `todo.db`. Other paths do not exist. Standard input may contain data (used by `import`).
- Only these system calls are available (Linux x86-64 numbers): `read` (0), `write` (1), `open` (2), `close` (3), `lseek` (8), `openat` (257), `exit` (60), `exit_group` (231). Any other system call ends the run as a failure.
- The program must end with `exit` or `exit_group`. Returning from the entry point is a failure.

## Data file `todo.db`

A sequence of 72-byte records, no header. Record layout:

| bytes | content |
|---|---|
| 0..4 | id, unsigned 32-bit little-endian, ≥ 1 |
| 4 | done flag: 0 or 1 |
| 5 | priority: 1, 2 or 3 (1 is the most important) |
| 6 | text length L, 1..64 |
| 7 | zero |
| 8..72 | the text in the first L bytes, zero bytes after it |

Records are in strictly increasing id order. A missing file means an empty list. At most 50 records. You may assume an existing file is well-formed and that every id is at most 4000000000.

## Terms

- **TEXT** is valid when it has 1 to 64 bytes. (All argument and input bytes are printable ASCII 0x20..0x7e, except newlines in standard input.)
- **ID** is valid when it is 1 to 10 ASCII digits (leading zeros allowed) with value at most 4294967295.
- **N** (priority) is valid when it is exactly one of the strings `1`, `2`, `3`.
- An **item line** is `<id> [ ] p<priority> <text>\n` for an open item and `<id> [x] p<priority> <text>\n` for a done item. Numbers are unsigned decimal without leading zeros.
- To **save** means to write the new list to `todo.db` (creating it if missing) so that it contains exactly the list in the format above.

## Commands

`argv[1]` selects the command; the table gives the accepted argument forms (after the command word). The words `-p`, `open` and `done` in these forms are literal (case-sensitive): another word in their place, any other number of arguments, an unknown command word, or no arguments at all is a usage error.

| command | effect on success | stdout on success |
|---|---|---|
| `add TEXT` | append a record: id = (id of the last record) + 1, or 1 if the list is empty; done 0; priority 2. Save. | `added <id>\n` |
| `add -p N TEXT` | the same with priority N | `added <id>\n` |
| `list` | no change | the item lines of all records, sorted by priority (1 first), records with equal priority in id order; `no items\n` if there are none |
| `list open` / `list done` | no change | the same, restricted to open (done 0) or done (done 1) records; `no items\n` if none is shown |
| `find WORD` | no change | the item lines, in file order, of the records whose text contains WORD as a substring, comparing ASCII letters case-insensitively (A–Z equals a–z); `no match\n` if there are none. WORD must be a valid TEXT. |
| `edit ID TEXT` | replace the text of that record (done flag and priority unchanged). Save. | `edited <id>\n` |
| `pri ID N` | set the priority of that record. Save. | `pri <id> <N>\n` |
| `done ID` | set the done flag of that record to 1 (also if already 1). Save. | `done <id>\n` |
| `undo ID` | set the done flag of that record to 0 (also if already 0). Save. | `undone <id>\n` |
| `rm ID` | remove that record. Save. | `removed <id>\n` |
| `clear` | remove every done record. Save, except when the file is missing (it stays missing). | `cleared <count removed>\n` |
| `stats` | no change | `total <n> open <o> done <d>\n` |
| `import` | read all of standard input; split it into lines at `\n` (a final line without `\n` also counts); ignore empty lines; append each remaining line, in order, as a new record (id as for `add`, done 0, priority 2). Save, unless no line was added (then nothing is written and a missing file stays missing). | `imported <count>\n` |

Standard input is at most 8192 bytes. Commands other than `import` must not depend on it.

## Errors

On an error nothing is written to stdout, the file is left exactly as it was (including whether it exists), the message goes to stderr, and the exit status is as given. The exit status is 0 on success.

Check in this order and report the first that applies:

| # | situation | stderr | exit |
|---|---|---|---|
| 1 | usage error (see above) | the usage message (below) | 2 |
| 2 | an argument TEXT or WORD is not valid | `error: bad text\n` | 1 |
| 3 | an argument N is not valid | `error: bad priority\n` | 1 |
| 4 | an argument ID is not valid | `error: bad id\n` | 1 |
| 5 | (`edit`, `pri`, `done`, `undo`, `rm`) no record has that id | `error: no such item\n` | 1 |
| 6 | (`import`) a non-empty line is longer than 64 bytes | `error: bad line <k>\n`, k = 1-based number of the line among all lines of the input (empty ones included) | 1 |
| 7 | (`add`, `import`) the result would have more than 50 records | `error: full\n` | 1 |

For `edit ID TEXT` check TEXT (2) before ID (4). For `pri ID N` check N (3) before ID (4). For `add -p N TEXT` check TEXT (2) before N (3). For `import`, check every line (6) before fullness (7).

The usage message is exactly this line followed by a newline (0x0a):

```
usage: todo add [-p N] TEXT | list [open|done] | find WORD | edit ID TEXT | pri ID N | done ID | undo ID | rm ID | clear | stats | import
```

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
