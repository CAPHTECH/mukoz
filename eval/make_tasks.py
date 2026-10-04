"""Generate eval/tasks/<task>/{spec.md, contract.toml, binding.toml, suite.toml}.

Contracts are written by hand here; the hidden oracle (oracle.py) has its own
independent Python reference for each task.
"""
import os, textwrap

COMMON_ABI = ("Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; "
              "return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. "
              "Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). "
              "The routine must not make system calls and must only touch the memory described below "
              "(plus its own stack below the return address).")

T = {}

T["abs_diff"] = dict(
 spec="uint64_t abs_diff(uint64_t a, uint64_t b): return |a - b| treating a and b as unsigned 64-bit integers.",
 contract='''
[inputs]
a = "bv64"
b = "bv64"

[results]
value = "bv64"

[[ensures]]
id = "value"
expr = "result.value == ite(uge(input.a, input.b), input.a - input.b, input.b - input.a)"
''',
 binding='''
[arguments]
rdi = "input.a"
rsi = "input.b"

[results]
value = "rax"
''')

T["smax"] = dict(
 spec="int64_t smax(int64_t a, int64_t b): return the larger of a and b as signed 64-bit integers.",
 contract='''
[inputs]
a = "bv64"
b = "bv64"

[results]
value = "bv64"

[[ensures]]
id = "value"
expr = "result.value == ite(sge(input.a, input.b), input.a, input.b)"
''',
 binding='''
[arguments]
rdi = "input.a"
rsi = "input.b"

[results]
value = "rax"
''')

T["popcount"] = dict(
 spec="uint64_t popcount(uint64_t x): return the number of 1 bits in x. (Any correct instruction sequence is fine.)",
 contract='''
[inputs]
x = "bv64"

[results]
value = "bv64"

[[ensures]]
id = "value"
expr = "result.value == (count i in bv64(0)..bv64(64): (lshr(input.x, i) & bv64(1)) == bv64(1))"
''',
 binding='''
[arguments]
rdi = "input.x"

[results]
value = "rax"
''')

T["sat_add_u32"] = dict(
 spec="uint32_t sat_add_u32(uint32_t a, uint32_t b): return a + b as unsigned 32-bit, saturating at 0xffffffff instead of wrapping. Only the low 32 bits of the return register (eax) are checked.",
 contract='''
[inputs]
a = "bv32"
b = "bv32"

[results]
value = "bv32"

[[ensures]]
id = "value"
expr = "result.value == ite(ult(input.a + input.b, input.a), bv32(0xffffffff), input.a + input.b)"
''',
 binding='''
[arguments]
rdi = "input.a"
rsi = "input.b"

[results]
value = "rax"
''')

T["fill"] = dict(
 spec="void fill(uint8_t *dst, uint8_t c, uint64_t n): set dst[0..n) to the byte c. dst points to exactly n writable bytes; c is passed in the low 8 bits of esi.",
 modifies='modifies = ["dst"]',
 contract='''
[inputs]
c = "bv8"

[state]
dst = { type = "bytes", max_len = 256 }

[[ensures]]
id = "filled"
expr = "len(after.dst) == len(before.dst) and (forall i in bv64(0)..len(before.dst): after.dst[i] == input.c)"
''',
 binding='''
[arguments]
rdi = "addr(dst)"
rsi = "input.c"
rdx = "len(before.dst)"

[regions.dst]
size = "len(before.dst)"
init = "before.dst"
access = "rw"
observe_as = "after.dst"
''')

T["count_byte"] = dict(
 spec="uint64_t count_byte(const uint8_t *buf, uint64_t n, uint8_t c): return how many of buf[0..n) equal c. buf points to exactly n readable bytes; c is passed in the low 8 bits of edx.",
 contract='''
[inputs]
buf = { type = "bytes", max_len = 256 }
c = "bv8"

[results]
value = "bv64"

[[ensures]]
id = "value"
expr = "result.value == (count i in bv64(0)..len(input.buf): input.buf[i] == input.c)"
''',
 binding='''
[arguments]
rdi = "addr(buf)"
rsi = "len(input.buf)"
rdx = "input.c"

[results]
value = "rax"

[regions.buf]
size = "len(input.buf)"
init = "input.buf"
access = "r"
''')

T["reverse"] = dict(
 spec="void reverse(uint8_t *buf, uint64_t n): reverse buf[0..n) in place. buf points to exactly n readable and writable bytes.",
 modifies='modifies = ["buf"]',
 contract='''
[state]
buf = { type = "bytes", max_len = 256 }

[[ensures]]
id = "reversed"
expr = "len(after.buf) == len(before.buf) and (forall i in bv64(0)..len(before.buf): after.buf[i] == before.buf[len(before.buf) - bv64(1) - i])"
''',
 binding='''
[arguments]
rdi = "addr(buf)"
rsi = "len(before.buf)"

[regions.buf]
size = "len(before.buf)"
init = "before.buf"
access = "rw"
observe_as = "after.buf"
''')

T["checked_mul"] = dict(
 spec="uint32_t checked_mul(uint64_t a, uint64_t b, uint64_t *out): if a*b fits in an unsigned 64-bit integer, store a*b to *out and return 0; otherwise leave *out unchanged and return 1. out points to 8 writable bytes (little-endian uint64). Only eax is checked for the return value.",
 modifies='modifies = ["out"]',
 contract='''
[inputs]
a = "bv64"
b = "bv64"

[state]
out = "bv64"

[results]
status = "bv32"

[[ensures]]
id = "status"
expr = "result.status == ite(ite(input.a == bv64(0), true, udiv(input.a * input.b, input.a) == input.b), bv32(0), bv32(1))"

[[ensures]]
id = "out"
expr = "ite(result.status == bv32(0), after.out == input.a * input.b, after.out == before.out)"
''',
 binding='''
[arguments]
rdi = "input.a"
rsi = "input.b"
rdx = "addr(out)"

[results]
status = "rax"

[regions.out]
size = "bv64(8)"
init = "before.out"
access = "rw"
observe_as = "after.out"
''')


for name, t in T.items():
    d = os.path.join("tasks", name)
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "spec.md"), "w") as f:
        f.write(f"# Task: {name}\n\n{t['spec']}\n\n{COMMON_ABI}\n\nDeliverable: a raw x86-64 machine-code file whose first byte is the entry point.\n")
    mod = t.get("modifies", "")
    with open(os.path.join(d, "contract.toml"), "w") as f:
        f.write(f'schema = "mukoz.contract/1"\nid = "task.{name}"\nboundary = "routine"\n{mod}\n' + t["contract"].lstrip("\n") +
                '\n[effects]\nallow = []\n\n[termination]\nkind = "must_return"\n')
    with open(os.path.join(d, "binding.toml"), "w") as f:
        f.write(f'schema = "mukoz.binding/1"\nid = "task.{name}@x86_64-sysv"\ncontract = "task.{name}"\ntarget = "x86_64/raw/sysv-x86_64/none"\n\n'
                '[entry]\nkind = "raw_offset"\noffset = 0\n' + t["binding"] + '\n[completion]\nkind = "return_to_sentinel"\n')
    with open(os.path.join(d, "suite.toml"), "w") as f:
        f.write(f'schema = "mukoz.suite/1"\nid = "task.{name}"\ncontract = "contract.toml"\nbinding = "binding.toml"\n\n'
                f'[generate]\nseed = "{name}"\nrandom_cases = 1024\n\n[limits]\ninstructions_per_case = 200000\n')
print("ok", sorted(T))
