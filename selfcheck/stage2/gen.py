#!/usr/bin/env python3
"""Self-check stage 2 table: the mukoz CLI as a process (docs/09 9.6). Each row is an invocation
and the expected exit code and stdout. Expected stdout follows the documented envelope (docs/07:
one pretty-printed JSON object with sorted keys); the expected values are written here by hand
and computed with Python, not with Mukoz."""
import json, os, struct
os.chdir(os.path.dirname(os.path.abspath(__file__)))

def env(command, data, ok, errors=None):
    return json.dumps({"api_version": "mukoz/1", "command": command, "data": data, "errors": errors or [], "ok": ok}, indent=2, sort_keys=True) + "\n"

ROWS = []  # (op, exit code, exact, expected stdout or prefix, expression text, file bytes)
# op 0: `mukoz expr check <text>`
for text, normalized in [
    ("bv8(1) + bv8(2)", "(bv8(0x01) + bv8(0x02))"),
    ("ult(input.a, input.b)", "ult(input.a, input.b)"),
    ("ite(true, bv16(5), bv16(6))", "ite(true, bv16(0x0005), bv16(0x0006))"),
    ("not (input.x == bv32(0))", "not (input.x == bv32(0x00000000))"),
]:
    ROWS.append((0, 0, 1, env("expr check", {"normalized": normalized}, True), text, b""))
err_prefix = '{\n  "api_version": "mukoz/1",\n  "command": "expr check",\n  "data": null,\n  "errors": [\n    {\n      "code": "CONTRACT_TYPE_ERROR",\n'
# `expr check` parses only (no type check, see `mukoz --help`), so the errors are syntax errors.
for text in ["bv8(1) +", "bv8(1) < bv8(2)", "ite("]:
    ROWS.append((0, 2, 0, err_prefix, text, b""))
# op 1: `mukoz inspect subject.bin` (prefix through head_hex: size, format and first bytes)
def inspect_prefix(b, fmt):
    return ('{\n  "api_version": "mukoz/1",\n  "command": "inspect",\n  "data": {\n'
            f'    "bytes": {len(b)},\n    "format": "{fmt}",\n    "head_hex": "{b[:64].hex()}",\n')
elf = b"\x7fELF" + bytes([2, 1, 1]) + bytes(9) + struct.pack("<HHIQQQIHHHHHH", 2, 62, 1, 0x401000, 64, 0, 0, 64, 56, 0, 64, 0, 0)
for b, fmt in [(bytes.fromhex("488d0437c3"), "raw"), (elf, "elf"), (struct.pack("<I", 0xfeedfacf) + bytes(28), "macho"), (b"MZ" + bytes(62), "pe")]:
    ROWS.append((1, 0, 0, inspect_prefix(b, fmt), "", b))

def row(op, code, exact, out, text, file):
    o, t = out.encode(), text.encode()
    return (struct.pack("<QQQQQQ", op, code, exact, len(o), len(t), len(file)) + o + t + file).hex()
rows = [row(*r) for r in ROWS]
H = 48
req = " or ".join(f'input.row == hex\\"{r}\\"' for r in rows)
open("contract.toml", "w").write(f'''schema = "mukoz.contract/1"
id = "selfcheck.cli"
boundary = "process"

# One row of the stage-2 table (gen.py): op (0 = expr check, 1 = inspect), expected exit code,
# whether the expected stdout is exact or a prefix, then the expected stdout, the expression
# text and the file contents.
[inputs]
row = {{ type = "bytes", max_len = 2048 }}
op = "bv64"
code = "bv64"
exact = "bv64"
expected = {{ type = "bytes", max_len = 1024 }}
text = {{ type = "bytes", max_len = 64 }}
file = {{ type = "bytes", max_len = 256 }}

[results]
status = "bv8"
out = {{ type = "bytes", max_len = 4096 }}
err = {{ type = "bytes", max_len = 1024 }}

[[requires]]
id = "row_from_table"
expr = "{req}"

[[requires]]
id = "fields_from_row"
expr = "input.op == u64le(input.row, bv64(0)) and input.code == u64le(input.row, bv64(8)) and input.exact == u64le(input.row, bv64(16)) and input.expected == slice(input.row, bv64({H}), u64le(input.row, bv64(24))) and input.text == slice(input.row, bv64({H}) + u64le(input.row, bv64(24)), u64le(input.row, bv64(32))) and input.file == slice(input.row, bv64({H}) + u64le(input.row, bv64(24)) + u64le(input.row, bv64(32)), u64le(input.row, bv64(40)))"

[[ensures]]
id = "exit_code"
expr = "result.status == extract(input.code, 7, 0)"

[[ensures]]
id = "stdout"
expr = "ite(uge(len(result.out), len(input.expected)), slice(result.out, bv64(0), len(input.expected)) == input.expected and (input.exact == bv64(0) or len(result.out) == len(input.expected)), false)"

[effects]
allow = ["read", "write", "open", "close"]

[termination]
kind = "must_exit"
''')
open("binding.toml", "w").write('''schema = "mukoz.binding/1"
id = "selfcheck.cli@x86_64.elf"
contract = "selfcheck.cli"
target = "x86_64/elf/sysv-x86_64/linux"

[entry]
kind = "elf_entry"

[process]
argv0 = "mukoz"
argv = ['ite(input.op == bv64(0), b"expr", b"inspect")', 'ite(input.op == bv64(0), b"check", b"subject.bin")', "input.text"]
argc = "ite(input.op == bv64(0), bv64(3), bv64(2))"

[files.subject]
path = "subject.bin"
init = "input.file"

[results]
status = "exit_status"
out = "stdout"
err = "stderr"

[completion]
kind = "exit"
''')
def guard(e, n):
    # Boundary rows are generated too; extraction is defined only for rows of the table
    # (the same membership test as `row_from_table`, which drops every other row).
    return f"ite({req}, {e}, {n})"
open("suite.toml", "w").write(f'''schema = "mukoz.suite/1"
id = "selfcheck.cli.native"
contract = "contract.toml"
binding = "binding.toml"
executors = ["native-process"]

[artifact]
path = "bin/mukoz-static"

[selfcheck]
checkers = "../checkers.toml"

[generate]
seed = "cli"
boundary = "product"
random_cases = 0

[generate.vars.row]
values = [{", ".join('"' + r + '"' for r in rows)}]

[generate.vars.op]
expr = "{guard("u64le(input.row, bv64(0))", "bv64(0)")}"

[generate.vars.code]
expr = "{guard("u64le(input.row, bv64(8))", "bv64(0)")}"

[generate.vars.exact]
expr = "{guard("u64le(input.row, bv64(16))", "bv64(0)")}"

[generate.vars.expected]
expr = "{guard(f"slice(input.row, bv64({H}), u64le(input.row, bv64(24)))", 'hex\\"\\"')}"

[generate.vars.text]
expr = "{guard(f"slice(input.row, bv64({H}) + u64le(input.row, bv64(24)), u64le(input.row, bv64(32)))", 'hex\\"\\"')}"

[generate.vars.file]
expr = "{guard(f"slice(input.row, bv64({H}) + u64le(input.row, bv64(24)) + u64le(input.row, bv64(32)), u64le(input.row, bv64(40)))", 'hex\\"\\"')}"

[limits]
wall_ms_per_case = 3000
''')
open("policy.toml", "w").write('''# Trial zone for self-check stage 2: the static mukoz built by build.sh.
[[native_trial_zones]]
id = "selfcheck-cli"
artifact_dir = "bin"
executors = ["native-process"]
targets = ["x86_64/elf/sysv-x86_64/linux"]
require_isolation = ["network_restriction", "filesystem_restriction", "resource_limits", "descendant_process_control", "credential_isolation"]
max_wall_ms_per_case = 3000
''')
print(f"{len(rows)} rows")
