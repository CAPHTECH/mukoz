#!/usr/bin/env python3
"""Self-check stage 3 (docs/09 9.6): compile selfcheck/kernels to x86_64 and aarch64 relocatable
objects and write the contracts, bindings and suites that check each kernel as a routine.

Where expected values come from (never Mukoz's expression evaluator for the bottom layer):
  bv kernels      a table computed below by Python, checked with `==` only; on x86_64 also the
                  real CPU (executors = emulated + native-routine)
  range_contains  a contract in the expression language (middle layer)
  elf_header      a table of hand-made headers and their codes, built below
"""
import hashlib, os, struct, subprocess, glob

os.chdir(os.path.dirname(os.path.abspath(__file__)))
M = (1 << 64) - 1
ISAS = {"x86_64": ("x86_64-unknown-linux-gnu", "x86_64/elf/sysv-x86_64/none", ["rdi", "rsi", "rdx", "rcx"], "rax"),
        "aarch64": ("aarch64-unknown-linux-gnu", "aarch64/elf/aapcs64/none", ["x0", "x1", "x2", "x3"], "x0")}

# ---------------------------------------------------------------- objects
os.makedirs("obj", exist_ok=True)
manifest = [f"# {subprocess.check_output(['rustc', '--version'], text=True).strip()}; flags: --edition 2024 -C opt-level=2 -C panic=abort -C relocation-model=pic"]
for isa, (triple, *_rest) in ISAS.items():
    out = f"obj/kernels_{isa}.o"
    subprocess.check_call(["rustc", "--edition", "2024", "--crate-type=lib", "--crate-name", "mukoz_kernels", "--emit=obj", "--target", triple,
                           "-C", "opt-level=2", "-C", "panic=abort", "-C", "relocation-model=pic", "-o", out, "../kernels/src/lib.rs"])
    b = open(out, "rb").read()
    manifest.append(f"{out} {len(b)} {hashlib.sha256(b).hexdigest()}")
open("obj/manifest.txt", "w").write("\n".join(manifest) + "\n")

def s64(x): return x - (1 << 64) if x >> 63 else x
BV = {
    "mk_bv_add64": lambda a, b: (a + b) & M,
    "mk_bv_ult": lambda a, b: int(a < b),
    "mk_bv_slt": lambda a, b: int(s64(a) < s64(b)),
    "mk_bv_shl": lambda a, n: 0 if n >= 64 else (a << n) & M,
    "mk_bv_sext": lambda x, w: x if w == 0 or w >= 64 else (s64((x << (64 - w)) & M) >> (64 - w)) & M,
}
PAIRS = [(0, 0), (1, 1), (M, 1), (1 << 63, M), ((1 << 63) - 1, 1), (0x123456789abcdef0, 0x0fedcba987654321), (5, 3), (3, 5),
         (M, 0), (0, M), (1 << 63, 1 << 63), (0x80, 8), (0xff, 8), (0x7fff, 16), (0x8000, 16), (0xdeadbeef, 32), (1, 63), (1, 64), (1, 65), (0x8000000000000000, 63)]

def w(path, text):
    open(path, "w").write(text.lstrip("\n"))

def binding(cid, bid, isa, symbol, args, results, regions=""):
    _, target, _, _ = ISAS[isa]
    a = "\n".join(f'{r} = "{e}"' for r, e in args)
    rs = "\n".join(f'{k} = "{v}"' for k, v in results)
    return f'''schema = "mukoz.binding/1"
id = "{bid}"
contract = "{cid}"
target = "{target}"

[entry]
kind = "object_symbol"
symbol = "{symbol}"

[arguments]
{a}

[results]
{rs}
{regions}
[completion]
kind = "return_to_sentinel"
'''

def suite(sid, binding_file, isa, gen, executors):
    ex = '["emulated", "native-routine"]' if executors else '["emulated"]'
    return f'''schema = "mukoz.suite/1"
id = "{sid}"
contract = "contract.toml"
binding = "{binding_file}"
executors = {ex}

[artifact]
path = "../obj/kernels_{isa}.o"

[selfcheck]
checkers = "../../checkers.toml"
{gen}
'''

# ---------------------------------------------------------------- bv kernels: known-answer table
os.makedirs("bv", exist_ok=True)
w("bv/contract.toml", '''
schema = "mukoz.contract/1"
id = "selfcheck.kat2"
boundary = "routine"

# One row of a known-answer table: a, b, expected (u64 each, little-endian). The rows come from
# selfcheck/stage3/build.py; the only expression used on the result is `==`.
[inputs]
row = { type = "bytes", max_len = 24 }
a = "bv64"
b = "bv64"
expected = "bv64"

[results]
value = "bv64"

[[requires]]
id = "row_from_table"
expr = "len(input.row) == bv64(24) and input.a == u64le(input.row, bv64(0)) and input.b == u64le(input.row, bv64(8)) and input.expected == u64le(input.row, bv64(16))"

[[ensures]]
id = "known_answer"
expr = "result.value == input.expected"

[effects]
allow = []

[termination]
kind = "must_return"
''')
for fn, ref in BV.items():
    rows = [struct.pack("<QQQ", a, b, ref(a, b)).hex() for a, b in PAIRS]
    req_rows = " or ".join(f'input.row == hex"{r}"' for r in rows)
    for isa, (_, _, regs, ret) in ISAS.items():
        bfile = f"binding.{fn}.{isa}.toml"
        w(f"bv/{bfile}", binding("selfcheck.kat2", f"selfcheck.kat2.{fn}@{isa}", isa, fn, [(regs[0], "input.a"), (regs[1], "input.b")], [("value", ret)]))
        gen = f'''
[generate]
seed = "{fn}"
boundary = "product"
random_cases = 64

[generate.vars.row]
len = "bv64(24)"
values = [{", ".join('"' + r + '"' for r in rows)}]
pieces = [{", ".join('"' + r + ':1"' for r in rows)}]

[generate.vars.a]
expr = "u64le(input.row, bv64(0))"

[generate.vars.b]
expr = "u64le(input.row, bv64(8))"

[generate.vars.expected]
expr = "u64le(input.row, bv64(16))"
'''
        w(f"bv/suite.{fn}.{isa}.toml", suite(f"selfcheck.{fn}.{isa}", bfile, isa, gen, isa == "x86_64"))

# ---------------------------------------------------------------- range_contains: contract
os.makedirs("range", exist_ok=True)
w("range/contract.toml", '''
schema = "mukoz.contract/1"
id = "selfcheck.range_contains"
boundary = "routine"

# addr = base + off (the binding forms it), so most generated accesses are near the range.
[inputs]
base = "bv64"
size = "bv64"
off = "bv64"
width = "bv64"

[results]
inside = "bv64"

[[ensures]]
id = "inside_iff_contained_without_wrap"
expr = """result.inside == ite(
    not ult(input.base + input.off + input.width, input.base + input.off)
    and not ult(input.base + input.size, input.base)
    and ule(input.base, input.base + input.off)
    and ule(input.base + input.off + input.width, input.base + input.size),
  bv64(1), bv64(0))"""

[effects]
allow = []

[termination]
kind = "must_return"
''')
for isa, (_, _, regs, ret) in ISAS.items():
    w(f"range/binding.{isa}.toml", binding("selfcheck.range_contains", f"selfcheck.range_contains@{isa}", isa, "mk_range_contains",
        [(regs[0], "input.base"), (regs[1], "input.size"), (regs[2], "input.base + input.off"), (regs[3], "input.width")], [("inside", ret)]))
    w(f"range/suite.{isa}.toml", suite(f"selfcheck.range_contains.{isa}", f"binding.{isa}.toml", isa, '''
[generate]
seed = "range"
boundary = "product"
random_cases = 1024
''', isa == "x86_64"))

# ---------------------------------------------------------------- ELF header check: hand table
def hdr(machine=62, ty=2, phoff=64, phentsize=56, phnum=1, total=64 + 56):
    h = bytearray(b"\x7fELF" + bytes([2, 1, 1]) + bytes(9))
    h += struct.pack("<HHIQQQIHHHHHH", ty, machine, 1, 0x401000, phoff, 0, 0, 64, phentsize, phnum, 64, 0, 0)
    return bytes(h) + bytes(max(0, total - len(h)))
def put(b, off, v): b = bytearray(b); b[off] = v; return bytes(b)
OK, SHORT, MAGIC, CLASS, DATA, TYPE, MACHINE, PHENTSIZE, PH_OUTSIDE = range(9)
TABLE = [
    (62, hdr(), OK), (183, hdr(machine=183), OK), (62, hdr(ty=3), OK),
    (62, hdr()[:40], SHORT), (62, hdr()[:63], SHORT),
    (62, put(hdr(), 1, ord("F")), MAGIC), (62, put(hdr(), 4, 1), CLASS), (62, put(hdr(), 5, 2), DATA),
    (62, hdr(ty=1), TYPE), (62, hdr(machine=183), MACHINE), (183, hdr(), MACHINE),
    (62, hdr(phentsize=32), PHENTSIZE), (62, hdr(phoff=0x1000), PH_OUTSIDE), (62, hdr(phoff=M - 8), PH_OUTSIDE),
    (62, hdr(phnum=3), PH_OUTSIDE), (62, hdr(phnum=3, total=64 + 3 * 56), OK), (62, hdr(phnum=0, total=64), OK),
]
rows = [(struct.pack("<QQ", code, mach) + h).hex() for mach, h, code in TABLE]
os.makedirs("elfhdr", exist_ok=True)
w("elfhdr/contract.toml", f'''
schema = "mukoz.contract/1"
id = "selfcheck.elf_header"
boundary = "routine"

# A row: expected code (u64), machine (u64), then the header bytes. Rows and codes are the hand
# table in selfcheck/stage3/build.py (0 ok, 1 short, 2 magic, 3 class, 4 data, 5 type, 6 machine,
# 7 phentsize, 8 program headers outside).
[inputs]
row = {{ type = "bytes", max_len = 256 }}
expected = "bv64"
machine = "bv64"
header = {{ type = "bytes", max_len = 240 }}

[results]
code = "bv64"

[[requires]]
id = "row_from_table"
expr = "{" or ".join(f'input.row == hex' + chr(92) + '"' + r + chr(92) + '"' for r in rows)}"

[[requires]]
id = "fields_from_row"
expr = "ite(uge(len(input.row), bv64(16)), input.expected == u64le(input.row, bv64(0)) and input.machine == u64le(input.row, bv64(8)) and input.header == slice(input.row, bv64(16), len(input.row) - bv64(16)), false)"

[[ensures]]
id = "known_code"
expr = "result.code == input.expected"

[effects]
allow = []

[termination]
kind = "must_return"
''')
for isa, (_, _, regs, ret) in ISAS.items():
    w(f"elfhdr/binding.{isa}.toml", binding("selfcheck.elf_header", f"selfcheck.elf_header@{isa}", isa, "mk_elf_header_check",
        [(regs[0], "addr(buf)"), (regs[1], "len(input.header)"), (regs[2], "input.machine")], [("code", ret)], '''
[regions.buf]
size = "len(input.header)"
init = "input.header"
access = "r"
'''))
    w(f"elfhdr/suite.{isa}.toml", suite(f"selfcheck.elf_header.{isa}", f"binding.{isa}.toml", isa, f'''
[generate]
seed = "elf"
boundary = "product"
random_cases = 0

[generate.vars.row]
values = [{", ".join('"' + r + '"' for r in rows)}]

# Boundary rows (empty, short) are generated too; `requires` drops every row not in the table,
# and `ite` keeps the field extraction defined for them.
[generate.vars.expected]
expr = "ite(uge(len(input.row), bv64(16)), u64le(input.row, bv64(0)), bv64(0))"

[generate.vars.machine]
expr = "ite(uge(len(input.row), bv64(16)), u64le(input.row, bv64(8)), bv64(0))"

[generate.vars.header]
expr = "ite(uge(len(input.row), bv64(16)), slice(input.row, bv64(16), len(input.row) - bv64(16)), hex\\"\\")"
''', isa == "x86_64"))

# ---------------------------------------------------------------- memcpy: must be UNRESOLVED_DEPENDENCY
os.makedirs("copy", exist_ok=True)
w("copy/contract.toml", '''
schema = "mukoz.contract/1"
id = "selfcheck.copy_bytes"
boundary = "routine"
modifies = ["dst"]

[inputs]
src = { type = "bytes", max_len = 64 }

[state]
dst = { type = "bytes", max_len = 64 }

[[requires]]
id = "same_len"
expr = "len(before.dst) == len(input.src)"

[[ensures]]
id = "copied"
expr = "after.dst == input.src"

[effects]
allow = []

[termination]
kind = "must_return"
''')
for isa, (_, _, regs, ret) in ISAS.items():
    w(f"copy/binding.{isa}.toml", binding("selfcheck.copy_bytes", f"selfcheck.copy_bytes@{isa}", isa, "mk_copy_bytes",
        [(regs[0], "addr(dst)"), (regs[1], "addr(src)"), (regs[2], "len(input.src)")], [], '''
[regions.dst]
size = "len(before.dst)"
init = "before.dst"
access = "rw"
observe_as = "after.dst"

[regions.src]
size = "len(input.src)"
init = "input.src"
access = "r"
''').replace("[results]\n\n", ""))
    w(f"copy/suite.{isa}.toml", suite(f"selfcheck.copy_bytes.{isa}", f"binding.{isa}.toml", isa, '''
[generate]
seed = "copy"
random_cases = 16

[generate.vars.dst]
len = "len(input.src)"
''', False))

w("policy.toml", '''
# Trial zone for self-check stage 3: the kernel objects built by build.py.
[[native_trial_zones]]
id = "selfcheck-kernels"
artifact_dir = "obj"
executors = ["native-routine"]
targets = ["x86_64/elf/sysv-x86_64/none"]
require_isolation = ["network_restriction", "filesystem_restriction", "resource_limits", "descendant_process_control", "credential_isolation"]
max_wall_ms_per_case = 1000
''')
print("\n".join(manifest))
