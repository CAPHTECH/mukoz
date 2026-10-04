"""Generate eval/tasks/<task>/{spec.md, contract.toml, binding.toml, suite.toml}.

Contracts are written by hand here; the hidden oracle (oracle.py) has its own
independent Python reference for each task.
"""
import os, re, textwrap

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


T["memmove"] = dict(
 spec="void memmove_(uint8_t *dst, const uint8_t *src, uint64_t n): copy n bytes from src to dst; the two ranges may overlap (both lie inside one buffer); the result must be as if the bytes were first copied to a temporary buffer. No return value.",
 modifies='modifies = ["buf"]',
 contract='''
[inputs]
n = "bv64"
doff = "bv64"
soff = "bv64"

[state]
buf = { type = "bytes", max_len = 128 }

[[requires]]
id = "in_bounds"
expr = "ule(input.n, len(before.buf)) and ule(input.doff, len(before.buf) - input.n) and ule(input.soff, len(before.buf) - input.n)"

[[ensures]]
id = "moved"
expr = "after.buf == concat(concat(slice(before.buf, bv64(0), input.doff), slice(before.buf, input.soff, input.n)), slice(before.buf, input.doff + input.n, len(before.buf) - input.doff - input.n))"
''',
 binding='''
[arguments]
rdi = "addr(buf) + input.doff"
rsi = "addr(buf) + input.soff"
rdx = "input.n"

[regions.buf]
size = "len(before.buf)"
init = "before.buf"
access = "rw"
observe_as = "after.buf"
''',
 suite_extra='''
[generate.vars.n]
max = "len(before.buf)"

[generate.vars.doff]
max = "len(before.buf) - input.n"

[generate.vars.soff]
max = "len(before.buf) - input.n"
''')

T["hex_encode"] = dict(
 spec="void hex_encode(uint8_t *dst, const uint8_t *src, uint64_t n): write the lowercase hexadecimal form of src[0..n) to dst[0..2n): dst[2i] is the high nibble digit and dst[2i+1] the low nibble digit of src[i] ('0'-'9','a'-'f'). dst points to exactly 2n writable bytes, src to n readable bytes.",
 modifies='modifies = ["dst"]',
 contract='''
[inputs]
src = { type = "bytes", max_len = 128 }

[state]
dst = { type = "bytes", max_len = 256 }

[[requires]]
id = "dst_len"
expr = "len(before.dst) == len(input.src) + len(input.src)"

[[ensures]]
id = "high_digits"
expr = "forall i in bv64(0)..len(input.src): after.dst[i + i] == ite(ult(lshr(input.src[i], bv8(4)), bv8(10)), lshr(input.src[i], bv8(4)) + bv8(0x30), lshr(input.src[i], bv8(4)) + bv8(0x57))"

[[ensures]]
id = "low_digits"
expr = "forall i in bv64(0)..len(input.src): after.dst[i + i + bv64(1)] == ite(ult(input.src[i] & bv8(0x0f), bv8(10)), (input.src[i] & bv8(0x0f)) + bv8(0x30), (input.src[i] & bv8(0x0f)) + bv8(0x57))"

[[ensures]]
id = "length"
expr = "len(after.dst) == len(before.dst)"
''',
 binding='''
[arguments]
rdi = "addr(dst)"
rsi = "addr(src)"
rdx = "len(input.src)"

[regions.dst]
size = "len(before.dst)"
init = "before.dst"
access = "rw"
observe_as = "after.dst"

[regions.src]
size = "len(input.src)"
init = "input.src"
access = "r"
''',
 suite_extra='''
[generate.vars.dst]
len = "len(input.src) + len(input.src)"
''')

T["shl_var"] = dict(
 spec="uint64_t shl_var(uint64_t x, uint64_t s): return x shifted left by s bits as a mathematical operation on 64-bit values: the result is 0 when s >= 64 (s is a full 64-bit value).",
 contract='''
[inputs]
x = "bv64"
s = "bv64"

[results]
value = "bv64"

[[ensures]]
id = "value"
expr = "result.value == shl(input.x, input.s)"
''',
 binding='''
[arguments]
rdi = "input.x"
rsi = "input.s"

[results]
value = "rax"
''',
 suite_extra='''
[generate.vars.s]
values = ["3", "31", "32", "63", "64", "65", "127", "128", "256", "0x100000000"]
''')

T["isqrt"] = dict(
 spec="uint64_t isqrt(uint64_t x): return floor(sqrt(x)) for an unsigned 64-bit x, i.e. the largest r with r*r <= x.",
 contract='''
[inputs]
x = "bv64"

[results]
value = "bv64"

[[ensures]]
id = "floor_sqrt"
expr = "ult(result.value, bv64(0x100000000)) and ule(result.value * result.value, input.x) and ite(result.value == bv64(0xffffffff), true, ult(input.x, (result.value + bv64(1)) * (result.value + bv64(1))))"
''',
 binding='''
[arguments]
rdi = "input.x"

[results]
value = "rax"
''',
 suite_extra='''
[generate.vars.x]
values = ["3", "4", "15", "16", "17", "0xfffffffe00000001", "0xfffffffe00000000", "0x3fffffffffffffff", "0x4000000000000000", "1000000"]
''')

T["base64"] = dict(
 spec="void base64_encode(uint8_t *dst, const uint8_t *src, uint64_t n): write the standard base64 encoding (RFC 4648 alphabet A-Z a-z 0-9 + /, with '=' padding) of src[0..n) to dst. dst points to exactly 4*ceil(n/3) writable bytes, src to n readable bytes. No terminating NUL.",
 modifies='modifies = ["dst"]',
 contract='\n[inputs]\nsrc = { type = "bytes", max_len = 96 }\n\n[state]\ndst = { type = "bytes", max_len = 128 }\n\n[[requires]]\nid = "dst_len"\nexpr = "len(before.dst) == udiv(len(input.src) + bv64(2), bv64(3)) * bv64(4)"\n\n[[ensures]]\nid = "char0"\nexpr = "forall g in bv64(0)..udiv(len(input.src) + bv64(2), bv64(3)): after.dst[(g + g + g + g)] == ite(ult(lshr(input.src[(g + g + g)], bv8(2)), bv8(26)), lshr(input.src[(g + g + g)], bv8(2)) + bv8(65), ite(ult(lshr(input.src[(g + g + g)], bv8(2)), bv8(52)), lshr(input.src[(g + g + g)], bv8(2)) + bv8(71), ite(ult(lshr(input.src[(g + g + g)], bv8(2)), bv8(62)), lshr(input.src[(g + g + g)], bv8(2)) - bv8(4), ite(lshr(input.src[(g + g + g)], bv8(2)) == bv8(62), bv8(43), bv8(47)))))"\n\n[[ensures]]\nid = "char1"\nexpr = "forall g in bv64(0)..udiv(len(input.src) + bv64(2), bv64(3)): after.dst[(g + g + g + g) + bv64(1)] == ite(ult((shl(input.src[(g + g + g)] & bv8(3), bv8(4)) | lshr(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)), bv8(4))), bv8(26)), (shl(input.src[(g + g + g)] & bv8(3), bv8(4)) | lshr(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)), bv8(4))) + bv8(65), ite(ult((shl(input.src[(g + g + g)] & bv8(3), bv8(4)) | lshr(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)), bv8(4))), bv8(52)), (shl(input.src[(g + g + g)] & bv8(3), bv8(4)) | lshr(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)), bv8(4))) + bv8(71), ite(ult((shl(input.src[(g + g + g)] & bv8(3), bv8(4)) | lshr(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)), bv8(4))), bv8(62)), (shl(input.src[(g + g + g)] & bv8(3), bv8(4)) | lshr(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)), bv8(4))) - bv8(4), ite((shl(input.src[(g + g + g)] & bv8(3), bv8(4)) | lshr(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)), bv8(4))) == bv8(62), bv8(43), bv8(47)))))"\n\n[[ensures]]\nid = "char2"\nexpr = "forall g in bv64(0)..udiv(len(input.src) + bv64(2), bv64(3)): after.dst[(g + g + g + g) + bv64(2)] == ite(ult((g + g + g) + bv64(1), len(input.src)), ite(ult((shl(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)) & bv8(15), bv8(2)) | lshr(ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)), bv8(6))), bv8(26)), (shl(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)) & bv8(15), bv8(2)) | lshr(ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)), bv8(6))) + bv8(65), ite(ult((shl(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)) & bv8(15), bv8(2)) | lshr(ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)), bv8(6))), bv8(52)), (shl(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)) & bv8(15), bv8(2)) | lshr(ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)), bv8(6))) + bv8(71), ite(ult((shl(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)) & bv8(15), bv8(2)) | lshr(ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)), bv8(6))), bv8(62)), (shl(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)) & bv8(15), bv8(2)) | lshr(ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)), bv8(6))) - bv8(4), ite((shl(ite(ult((g + g + g) + bv64(1), len(input.src)), input.src[(g + g + g) + bv64(1)], bv8(0)) & bv8(15), bv8(2)) | lshr(ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)), bv8(6))) == bv8(62), bv8(43), bv8(47))))), bv8(61))"\n\n[[ensures]]\nid = "char3"\nexpr = "forall g in bv64(0)..udiv(len(input.src) + bv64(2), bv64(3)): after.dst[(g + g + g + g) + bv64(3)] == ite(ult((g + g + g) + bv64(2), len(input.src)), ite(ult((ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)) & bv8(63)), bv8(26)), (ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)) & bv8(63)) + bv8(65), ite(ult((ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)) & bv8(63)), bv8(52)), (ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)) & bv8(63)) + bv8(71), ite(ult((ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)) & bv8(63)), bv8(62)), (ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)) & bv8(63)) - bv8(4), ite((ite(ult((g + g + g) + bv64(2), len(input.src)), input.src[(g + g + g) + bv64(2)], bv8(0)) & bv8(63)) == bv8(62), bv8(43), bv8(47))))), bv8(61))"\n\n[[ensures]]\nid = "length"\nexpr = "len(after.dst) == len(before.dst)"\n',
 binding='''
[arguments]
rdi = "addr(dst)"
rsi = "addr(src)"
rdx = "len(input.src)"

[regions.dst]
size = "len(before.dst)"
init = "before.dst"
access = "rw"
observe_as = "after.dst"

[regions.src]
size = "len(input.src)"
init = "input.src"
access = "r"
''',
 suite_extra='''
[generate.vars.dst]
len = "udiv(len(input.src) + bv64(2), bv64(3)) * bv64(4)"
''')

T["count_byte_fast"] = dict(T["count_byte"])
T["count_byte_fast"]["spec"] = T["count_byte"]["spec"].replace("uint64_t count_byte(", "uint64_t count_byte_fast(") + (
    " Performance requirement: process the buffer 8 bytes per load (64-bit loads combined with word-at-a-time / SWAR bit tricks); "
    "a loop that handles every byte individually is not acceptable. At most 7 leftover bytes at the end may be handled one at a time. "
    "Unaligned 64-bit loads are allowed, but never read outside buf[0..n).")

# --- utf8_count: per-position formulation (UTF-8 is self-synchronizing), no fold needed.
def _u8():
    b = lambda i: f"input.buf[{i}]"
    cont = lambda x: f"(({x} & bv8(0xc0)) == bv8(0x80))"
    ll = lambda x: (f"ite(ult({x}, bv8(0x80)), bv64(1), ite(ult({x}, bv8(0xc2)), bv64(0), ite(ult({x}, bv8(0xe0)), bv64(2), "
                    f"ite(ult({x}, bv8(0xf0)), bv64(3), ite(ult({x}, bv8(0xf5)), bv64(4), bv64(0))))))")
    lo2 = lambda x: f"ite({x} == bv8(0xe0), bv8(0xa0), ite({x} == bv8(0xf0), bv8(0x90), bv8(0x80)))"
    hi2 = lambda x: f"ite({x} == bv8(0xed), bv8(0x9f), ite({x} == bv8(0xf4), bv8(0x8f), bv8(0xbf)))"
    n = "len(input.buf)"
    x = b("i")
    k = ll(x)
    # Lead (non-continuation) at i: known length, fits, second byte in range, rest are continuations.
    lead = (f"ite({k} == bv64(0), false, ite(ugt(i + {k}, {n}), false, ite(ult({k}, bv64(2)), true, "
            f"ite(ult({b('i + bv64(1)')}, {lo2(x)}), false, ite(ugt({b('i + bv64(1)')}, {hi2(x)}), false, "
            f"ite(ult({k}, bv64(3)), true, ite(not {cont(b('i + bv64(2)'))}, false, "
            f"ite(ult({k}, bv64(4)), true, {cont(b('i + bv64(3)'))}))))))))")
    def back(d, rest):
        y = b(f"i - bv64({d})")
        return f"ite(ult(i, bv64({d})), false, ite(not {cont(y)}, ugt({ll(y)}, bv64({d})), {rest}))"
    contin = back(1, back(2, back(3, "false")))
    valid = f"(forall i in bv64(0)..{n}: ite({cont(x)}, {contin}, {lead}))"
    count = f"(count i in bv64(0)..{n}: not {cont(x)})"
    return valid, count
_valid, _count = _u8()
_V = ["41", "7f", "00", "20", "4142434445464748", "30313233343536373839", "7f00412020207e", "6162636465666768696a6b6c6d6e6f70", "c280", "dfbf", "c3a9", "e0a080", "efbfbf", "ed9fbf", "ee8080", "e38182", "f0908080", "f48fbfbf", "f1808080", "f09f9880"]
_I = ["80", "bf", "c080", "c1bf", "e09fbf", "eda080", "edbfbf", "f08fbfbf", "f4908080", "f5808080", "ff", "fe", "c2", "e180", "f09080"]
T["utf8_count"] = dict(
 spec="uint64_t utf8_count(const uint8_t *buf, uint64_t n): if buf[0..n) is well-formed UTF-8 (RFC 3629 / Unicode Table 3-7: "
      "no overlong forms, no surrogates U+D800-U+DFFF, nothing above U+10FFFF, no truncated or stray bytes), return the number of "
      "code points; otherwise return 0xffffffffffffffff. buf points to exactly n readable bytes (n may be 0).",
 contract=f"""
[inputs]
buf = {{ type = "bytes", max_len = 256 }}

[results]
value = "bv64"

[[ensures]]
id = "valid_count"
expr = "ite({_valid}, result.value == {_count}, true)"

[[ensures]]
id = "invalid_all_ones"
expr = "ite({_valid}, true, result.value == bv64(0xffffffffffffffff))"
""",
 binding="""
[arguments]
rdi = "addr(buf)"
rsi = "len(input.buf)"

[results]
value = "rax"

[regions.buf]
size = "len(input.buf)"
init = "input.buf"
access = "r"
""",
 suite_extra="\n[generate.vars.buf]\npieces = [" + ", ".join([f'"{v}:100"' for v in _V] + [f'"{v}:1"' for v in _I]) + "]\n")

# --- utf8_to_utf16: validity as in utf8_count; output position from counts of earlier leads.
def _u16():
    b = lambda i: f"input.src[{i}]"
    cont = lambda x: f"(({x} & bv8(0xc0)) == bv8(0x80))"
    ll = lambda x: (f"ite(ult({x}, bv8(0x80)), bv64(1), ite(ult({x}, bv8(0xc2)), bv64(0), ite(ult({x}, bv8(0xe0)), bv64(2), "
                    f"ite(ult({x}, bv8(0xf0)), bv64(3), ite(ult({x}, bv8(0xf5)), bv64(4), bv64(0))))))")
    lo2 = lambda x: f"ite({x} == bv8(0xe0), bv8(0xa0), ite({x} == bv8(0xf0), bv8(0x90), bv8(0x80)))"
    hi2 = lambda x: f"ite({x} == bv8(0xed), bv8(0x9f), ite({x} == bv8(0xf4), bv8(0x8f), bv8(0xbf)))"
    n = "len(input.src)"
    x = b("i"); k = ll(x)
    lead = (f"ite({k} == bv64(0), false, ite(ugt(i + {k}, {n}), false, ite(ult({k}, bv64(2)), true, "
            f"ite(ult({b('i + bv64(1)')}, {lo2(x)}), false, ite(ugt({b('i + bv64(1)')}, {hi2(x)}), false, "
            f"ite(ult({k}, bv64(3)), true, ite(not {cont(b('i + bv64(2)'))}, false, "
            f"ite(ult({k}, bv64(4)), true, {cont(b('i + bv64(3)'))}))))))))")
    def back(d, rest):
        y = b(f"i - bv64({d})")
        return f"ite(ult(i, bv64({d})), false, ite(not {cont(y)}, ugt({ll(y)}, bv64({d})), {rest}))"
    valid = f"(forall i in bv64(0)..{n}: ite({cont(x)}, {back(1, back(2, back(3, 'false')))}, {lead}))"
    units = f"((count i in bv64(0)..{n}: not {cont(x)}) + (count i in bv64(0)..{n}: uge({x}, bv8(0xf0))))"
    z = lambda e: f"zext({e}, 32)"
    c6 = lambda e, sh: f"shl({z(e)} & bv32(0x3f), bv32({sh}))" if sh else f"({z(e)} & bv32(0x3f))"
    b1, b2, b3 = b("i + bv64(1)"), b("i + bv64(2)"), b("i + bv64(3)")
    cp = (f"ite(ult({x}, bv8(0x80)), {z(x)}, ite(ult({x}, bv8(0xe0)), shl({z(x)} & bv32(0x1f), bv32(6)) | {c6(b1, 0)}, "
          f"ite(ult({x}, bv8(0xf0)), shl({z(x)} & bv32(0x0f), bv32(12)) | {c6(b1, 6)} | {c6(b2, 0)}, "
          f"shl({z(x)} & bv32(0x07), bv32(18)) | {c6(b1, 12)} | {c6(b2, 6)} | {c6(b3, 0)})))")
    pos = f"((count j in bv64(0)..i: not {cont(b('j'))}) + (count j in bv64(0)..i: uge({b('j')}, bv8(0xf0))))"
    p = f"({pos} + {pos})"
    def unit_at(off, u):
        return f"(after.dst[{p} + bv64({off})] == extract({u}, 7, 0) and after.dst[{p} + bv64({off + 1})] == extract({u}, 15, 8))"
    s4 = f"({cp} - bv32(0x10000))"
    hi = f"(bv32(0xd800) + lshr({s4}, bv32(10)))"
    lo = f"(bv32(0xdc00) + ({s4} & bv32(0x3ff)))"
    unit_ok = f"ite(ult({x}, bv8(0xf0)), {unit_at(0, cp)}, {unit_at(0, hi)} and {unit_at(2, lo)})"
    out = f"(forall i in bv64(0)..{n}: ite({cont(x)}, true, {unit_ok}))"
    frame = f"(forall j in {units} + {units}..len(before.dst): after.dst[j] == before.dst[j])"
    return valid, units, out, frame
_v16, _n16, _o16, _f16 = _u16()
T["utf8_to_utf16"] = dict(
 spec="uint64_t utf8_to_utf16(uint16_t *dst, const uint8_t *src, uint64_t n): if src[0..n) is well-formed UTF-8 (RFC 3629 / Unicode Table 3-7: no overlong forms, no surrogates U+D800-U+DFFF, nothing above U+10FFFF, no truncated or stray bytes), write its UTF-16LE encoding to dst (code points above U+FFFF as surrogate pairs) and return the number of 16-bit units written. Otherwise return 0xffffffffffffffff (dst contents are then unspecified). dst has room for n units (2n bytes) and src points to exactly n readable bytes (n may be 0). Only dst[0..n) may be written, and for valid input nothing beyond the returned number of units may be modified.",
 modifies='modifies = ["dst"]',
 contract=f"""
[inputs]
src = {{ type = "bytes", max_len = 128 }}

[state]
dst = {{ type = "bytes", max_len = 256 }}

[results]
value = "bv64"

[[requires]]
id = "dst_room"
expr = "len(before.dst) == len(input.src) + len(input.src)"

[[ensures]]
id = "count"
expr = "ite({_v16}, result.value == {_n16}, true)"

[[ensures]]
id = "invalid_all_ones"
expr = "ite({_v16}, true, result.value == bv64(0xffffffffffffffff))"

[[ensures]]
id = "units"
expr = "ite({_v16}, {_o16}, true)"

[[ensures]]
id = "rest_unchanged"
expr = "ite({_v16}, {_f16}, true)"
""",
 binding="""
[arguments]
rdi = "addr(dst)"
rsi = "addr(src)"
rdx = "len(input.src)"

[results]
value = "rax"

[regions.dst]
size = "len(before.dst)"
init = "before.dst"
access = "rw"
observe_as = "after.dst"

[regions.src]
size = "len(input.src)"
init = "input.src"
access = "r"
""",
 suite_extra="\n[generate.vars.src]\npieces = [" + ", ".join([f'"{v}:100"' for v in _V] + [f'"{v}:1"' for v in _I]) + "]\n\n[generate.vars.dst]\nlen = \"len(input.src) + len(input.src)\"\n")

T["utf8_to_utf16_fast"] = dict(T["utf8_to_utf16"])
T["utf8_to_utf16_fast"]["spec"] = T["utf8_to_utf16"]["spec"].replace("uint64_t utf8_to_utf16(", "uint64_t utf8_to_utf16_fast(") + (
    " Performance requirement: whenever at least 8 input bytes remain and the next 8 bytes are all ASCII (< 0x80), they must be "
    "handled together: one 64-bit load, an all-ASCII test on the whole word, and the 8 resulting units written with 64-bit (or wider) "
    "stores. Byte-at-a-time handling is only allowed for non-ASCII sequences and when fewer than 8 bytes remain or the next 8 are not all ASCII. "
    "Unaligned loads and stores are allowed.")

# --- codec: four converters behind one entry. Each op reads its own generated input variable; the
# binding hands the selected one to the routine as src.
_M = "bv64(0xffffffffffffffff)"
def _codec():
    v8, v16, vr, vb = "input.s8", "input.s16", "input.sraw", "input.sb64"
    S = f"ite(input.op == bv64(0), {v8}, ite(input.op == bv64(1), {v16}, ite(input.op == bv64(3), {vb}, {vr})))"
    B = lambda e: f"extract({e}, 7, 0)"
    z = lambda e: f"zext({e}, 32)"
    # op 0
    va, na, oa, fa = (x.replace("input.src", v8) for x in _u16())
    # op 1: UTF-16LE -> UTF-8
    n1 = f"len({v16})"; m1 = f"udiv({n1}, bv64(2))"
    hb = lambda k: f"{v16}[{k} + {k} + bv64(1)]"
    lb = lambda k: f"{v16}[{k} + {k}]"
    isH = lambda k: f"(({hb(k)} & bv8(0xfc)) == bv8(0xd8))"
    isL = lambda k: f"(({hb(k)} & bv8(0xfc)) == bv8(0xdc))"
    u = lambda k: f"({z(lb(k))} | shl({z(hb(k))}, bv32(8)))"
    vb1 = (f"(urem({n1}, bv64(2)) == bv64(0) and (forall k in bv64(0)..{m1}: ite({isH('k')}, ite(ult(k + bv64(1), {m1}), {isL('k + bv64(1)')}, false), "
           f"ite({isL('k')}, ite(k == bv64(0), false, {isH('k - bv64(1)')}), true))))")
    def len1(rng, j):
        return (f"((count {j} in {rng}: not {isL(j)}) + (count {j} in {rng}: uge({u(j)}, bv32(0x80)) and not {isL(j)}) + "
                f"(count {j} in {rng}: uge({u(j)}, bv32(0x800)) and not {isL(j)}) + (count {j} in {rng}: {isH(j)}))")
    nb1 = len1(f"bv64(0)..{m1}", "j")
    p1 = len1("bv64(0)..k", "j")
    at = lambda off, e: f"after.dst[{p1} + bv64({off})] == {B(e)}"
    uk = u("k")
    cp4 = f"(bv32(0x10000) + shl({uk} - bv32(0xd800), bv32(10)) + ({u('k + bv64(1)')} - bv32(0xdc00)))"
    enc1 = (f"ite(ult({uk}, bv32(0x80)), {at(0, uk)}, ite(ult({uk}, bv32(0x800)), {at(0, f'bv32(0xc0) | lshr({uk}, bv32(6))')} and {at(1, f'bv32(0x80) | ({uk} & bv32(0x3f))')}, "
            f"ite(not {isH('k')}, {at(0, f'bv32(0xe0) | lshr({uk}, bv32(12))')} and {at(1, f'bv32(0x80) | (lshr({uk}, bv32(6)) & bv32(0x3f))')} and {at(2, f'bv32(0x80) | ({uk} & bv32(0x3f))')}, "
            f"{at(0, f'bv32(0xf0) | lshr({cp4}, bv32(18))')} and {at(1, f'bv32(0x80) | (lshr({cp4}, bv32(12)) & bv32(0x3f))')} and "
            f"{at(2, f'bv32(0x80) | (lshr({cp4}, bv32(6)) & bv32(0x3f))')} and {at(3, f'bv32(0x80) | ({cp4} & bv32(0x3f))')})))")
    ob1 = f"(forall k in bv64(0)..{m1}: ite({isL('k')}, true, {enc1}))"
    # op 2: base64 encode
    n2 = f"len({vr})"; G = f"udiv({n2} + bv64(2), bv64(3))"
    T = lambda x: (f"ite(ult({x}, bv32(26)), {x} + bv32(0x41), ite(ult({x}, bv32(52)), {x} + bv32(71), "
                   f"ite(ult({x}, bv32(62)), {x} - bv32(4), ite({x} == bv32(62), bv32(0x2b), bv32(0x2f)))))")
    i2 = "(g * bv64(3))"
    sb = lambda d: f"ite(ult({i2} + bv64({d}), {n2}), {z(f'{vr}[{i2} + bv64({d})]')}, bv32(0))"
    w = f"(shl({sb(0)}, bv32(16)) | shl({sb(1)}, bv32(8)) | {sb(2)})"
    o2 = lambda d, e: f"after.dst[g * bv64(4) + bv64({d})] == {B(e)}"
    ob2 = (f"(forall g in bv64(0)..{G}: {o2(0, T(f'lshr({w}, bv32(18))'))} and {o2(1, T(f'(lshr({w}, bv32(12)) & bv32(63))'))} and "
           f"{o2(2, f'ite(ult({i2} + bv64(1), {n2}), {T(f"(lshr({w}, bv32(6)) & bv32(63))")}, bv32(0x3d))')} and "
           f"{o2(3, f'ite(ult({i2} + bv64(2), {n2}), {T(f"({w} & bv32(63))")}, bv32(0x3d))')})")
    nb2 = f"({G} * bv64(4))"
    # op 3: strict base64 decode
    n3 = f"len({vb})"
    isA = lambda c: (f"((uge({c}, bv8(0x41)) and ule({c}, bv8(0x5a))) or (uge({c}, bv8(0x61)) and ule({c}, bv8(0x7a))) or "
                     f"(uge({c}, bv8(0x30)) and ule({c}, bv8(0x39))) or {c} == bv8(0x2b) or {c} == bv8(0x2f))")
    D = lambda c: (f"ite(ule({c}, bv8(0x2b)), bv32(62), ite(ule({c}, bv8(0x2f)), bv32(63), ite(ule({c}, bv8(0x39)), {z(c)} + bv32(4), "
                   f"ite(ule({c}, bv8(0x5a)), {z(c)} - bv32(0x41), {z(c)} - bv32(71)))))")
    eq = lambda c: f"({c} == bv8(0x3d))"
    s3 = lambda i: f"{vb}[{i}]"
    P2 = f"ite(ult({n3}, bv64(4)), false, {eq(s3(f'{n3} - bv64(2)'))})"
    P1 = f"ite(ult({n3}, bv64(4)), false, {eq(s3(f'{n3} - bv64(1)'))} and not {eq(s3(f'{n3} - bv64(2)'))})"
    vb3 = (f"(urem({n3}, bv64(4)) == bv64(0) and (forall i in bv64(0)..{n3}: ite({isA(s3('i'))}, true, "
           f"{eq(s3('i'))} and (i + bv64(1) == {n3} or (i + bv64(2) == {n3} and {eq(s3(f'{n3} - bv64(1)'))})))) and "
           f"ite({P2}, ({D(s3(f'{n3} - bv64(3)'))} & bv32(15)) == bv32(0), ite({P1}, ({D(s3(f'{n3} - bv64(2)'))} & bv32(3)) == bv32(0), true)))")
    nb3 = f"(udiv({n3}, bv64(4)) * bv64(3) - (count i in bv64(0)..{n3}: {eq(s3('i'))}))"
    q = lambda d: D(s3(f"g * bv64(4) + bv64({d})"))
    o3 = lambda d, e: f"after.dst[g * bv64(3) + bv64({d})] == {B(e)}"
    ob3 = (f"(forall g in bv64(0)..udiv({n3}, bv64(4)): {o3(0, f'shl({q(0)}, bv32(2)) | lshr({q(1)}, bv32(4))')} and "
           f"ite({eq(s3('g * bv64(4) + bv64(2)'))}, true, {o3(1, f'shl({q(1)} & bv32(15), bv32(4)) | lshr({q(2)}, bv32(2))')}) and "
           f"ite({eq(s3('g * bv64(4) + bv64(3)'))}, true, {o3(2, f'shl({q(2)} & bv32(3), bv32(6)) | {q(3)}')}))")
    fr = lambda k: f"(forall j in {k}..len(before.dst): after.dst[j] == before.dst[j])"
    ops = [(va, na, oa, fr(f"{na} + {na}")), (vb1, nb1, ob1, fr(nb1)), ("true", nb2, ob2, fr(nb2)), (vb3, nb3, ob3, fr(nb3))]
    ens = ""
    for k, (V, N, O, F) in enumerate(ops):
        sel = f"input.op == bv64({k})"
        ens += (f'\n[[ensures]]\nid = "op{k}_result"\nexpr = "ite({sel}, ite({V}, result.value == {N}, result.value == {_M}), true)"\n'
                f'\n[[ensures]]\nid = "op{k}_output"\nexpr = "ite({sel}, ite({V}, {O}, true), true)"\n'
                f'\n[[ensures]]\nid = "op{k}_rest_unchanged"\nexpr = "ite({sel}, ite({V}, {F}, true), true)"\n')
    ens += f'\n[[ensures]]\nid = "bad_op"\nexpr = "ite(ugt(input.op, bv64(3)), result.value == {_M}, true)"\n'
    return S, ens
_S, _ens = _codec()
_P16 = ["4100:30", "7e00:30", "4100420043004400:30", "e900:20", "ff07:10", "0008:10", "ac20:10", "ffff:10", "ffd7:10", "00e0:10",
        "3dd800de:20", "00d800dc:10", "ffdbffdf:10", "00d8:1", "ffdb:1", "00dc:1", "ffdf:1", "00dc00d8:1", "00d84100:1"]
_P64 = ["51554a44:30", "59574a6a:30", "41414141:10", "2f2f2f2f:10", "2b2f2b2f:10", "41503841:10", "51513d3d:8", "5155493d:8",
        "51523d3d:4", "51554a3d:4", "41423d3d:4", "4142433d:4", "413d3d3d:1", "3d3d3d3d:1", "41413d41:1", "3d414141:1", "41412a41:1", "80414141:1", "41410a41:1"]
T["codec"] = dict(
 spec='uint64_t codec(uint64_t op, uint8_t *dst, const uint8_t *src, uint64_t n) — a converter selected by op. src points to exactly n readable bytes (n may be 0); dst points to 2n+4 writable bytes. On success it writes the output to dst[0..k) and returns k as described below, and must not modify dst beyond the output. On invalid input, or if op is not 0-3, it returns 0xffffffffffffffff (dst contents are then unspecified, but only dst[0..2n+4) may be written).\n\n- op 0, UTF-8 to UTF-16LE: src must be well-formed UTF-8 (RFC 3629 / Unicode Table 3-7: no overlong forms, no surrogates U+D800-U+DFFF, nothing above U+10FFFF, no truncated or stray bytes). Output: the UTF-16LE encoding (code points above U+FFFF as surrogate pairs). Returns the number of 16-bit units (the output has 2*k bytes in that case; "dst beyond the output" means beyond those 2*k bytes).\n- op 1, UTF-16LE to UTF-8: n must be even and src must be well-formed UTF-16LE (every high surrogate D800-DBFF immediately followed by a low surrogate DC00-DFFF, no unpaired low surrogate). Output: the UTF-8 encoding. Returns the number of bytes.\n- op 2, base64 encode (RFC 4648 alphabet A-Z a-z 0-9 + /, with = padding). Returns the number of bytes (4*ceil(n/3)).\n- op 3, base64 decode, strict: n must be a multiple of 4; only alphabet characters, except that the last group may end in "=" or "=="; the unused bits before padding must be zero (canonical encoding). Output: the decoded bytes. Returns their number.',
 modifies='modifies = ["dst"]',
 contract=f"""
[inputs]
op = {{ type = "bv64" }}
s8 = {{ type = "bytes", max_len = 96 }}
k16 = {{ type = "bv64" }}
s16 = {{ type = "bytes", max_len = 97 }}
sraw = {{ type = "bytes", max_len = 96 }}
k64 = {{ type = "bv64" }}
sb64 = {{ type = "bytes", max_len = 97 }}

[state]
dst = {{ type = "bytes", max_len = 200 }}

[results]
value = "bv64"

[[requires]]
id = "dst_room"
expr = "len(before.dst) == len({_S}) + len({_S}) + bv64(4)"
{_ens}""",
 binding=f"""
[arguments]
rdi = "input.op"
rsi = "addr(dst)"
rdx = "addr(src)"
rcx = "len({_S})"

[results]
value = "rax"

[regions.dst]
size = "len(before.dst)"
init = "before.dst"
access = "rw"
observe_as = "after.dst"

[regions.src]
size = "len({_S})"
init = "{_S}"
access = "r"
""",
 suite_extra="\n[generate.vars.op]\nmax = \"bv64(4)\"\nvalues = [\"0xffffffffffffffff\", \"0x100000000\", \"0x100000003\"]\n"
   "\n[generate.vars.s8]\npieces = [" + ", ".join([f'"{v}:100"' for v in _V] + [f'"{v}:1"' for v in _I]) + "]\n"
   "\n[generate.vars.k16]\nmax = \"bv64(96)\"\n"
   "\n[generate.vars.s16]\nlen = \"(input.k16 & bv64(0xfffe)) + ite(urem(input.k16, bv64(7)) == bv64(0), bv64(1), bv64(0))\"\npieces = [" + ", ".join(f'"{v}"' for v in _P16) + "]\n"
   "\n[generate.vars.k64]\nmax = \"bv64(96)\"\n"
   "\n[generate.vars.sb64]\nlen = \"(input.k64 & bv64(0xfffc)) + ite(urem(input.k64, bv64(9)) == bv64(0), bv64(1), bv64(0))\"\npieces = [" + ", ".join(f'"{v}"' for v in _P64) + "]\n"
   f"\n[generate.vars.dst]\nlen = \"len({_S}) + len({_S}) + bv64(4)\"\n")

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
                f'[generate]\nseed = "{name}"\nrandom_cases = 1024\n' + t.get("suite_extra", "") + '\n[limits]\ninstructions_per_case = 200000\n')
print("ok", sorted(T))

# --- aarch64 variants (cross-ISA experiment): same contracts, AAPCS64 bindings.
A64_ABI = ("Target CPU: AArch64 (ARMv8-A, little-endian). Calling convention: AAPCS64. Arguments in x0-x7 (w0-w7 for 32-bit and narrower), "
           "return value in x0 (or w0). Return with RET to the address in x30. Preserve x19-x29 and sp. "
           "Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). "
           "Use only base integer A64 instructions (no SIMD/FP). The routine must not make system calls and must only touch the memory "
           "described below (plus its own stack below the incoming sp).")
A64_REG = {"rdi": "x0", "rsi": "x1", "rdx": "x2", "rcx": "x3", "r8": "x4", "r9": "x5", "rax": "x0"}
A64_TASKS = ["count_byte", "memmove", "isqrt", "hex_encode", "base64", "count_byte_fast", "utf8_count", "utf8_to_utf16", "utf8_to_utf16_fast", "codec"]
for name in A64_TASKS:
    t = T[name]
    d = os.path.join("tasks", name + "_a64")
    os.makedirs(d, exist_ok=True)
    spec = t["spec"].replace("low 8 bits of edx", "low 8 bits of w2")
    with open(os.path.join(d, "spec.md"), "w") as f:
        f.write(f"# Task: {name} (AArch64)\n\n{spec}\n\n{A64_ABI}\n\nDeliverable: a raw AArch64 machine-code file (little-endian 32-bit instruction words) whose first instruction is the entry point.\n")
    with open(os.path.join("tasks", name, "contract.toml")) as f:
        contract = f.read()
    with open(os.path.join(d, "contract.toml"), "w") as f:
        f.write(contract)
    binding = t["binding"]
    for x, a in A64_REG.items():
        binding = re.sub(rf'^{x} =', f'{a} =', binding, flags=re.M)
        binding = re.sub(rf'= "{x}"', f'= "{a}"', binding)
    with open(os.path.join(d, "binding.toml"), "w") as f:
        f.write(f'schema = "mukoz.binding/1"\nid = "task.{name}@aarch64-aapcs64"\ncontract = "task.{name}"\ntarget = "aarch64/raw/aapcs64/none"\n\n'
                '[entry]\nkind = "raw_offset"\noffset = 0\n' + binding + '\n[completion]\nkind = "return_to_sentinel"\n')
    with open(os.path.join(d, "suite.toml"), "w") as f:
        f.write(f'schema = "mukoz.suite/1"\nid = "task.{name}_a64"\ncontract = "contract.toml"\nbinding = "binding.toml"\n\n'
                f'[generate]\nseed = "{name}"\nrandom_cases = 1024\n' + t.get("suite_extra", "") + '\n[limits]\ninstructions_per_case = 200000\n')
print("ok a64", A64_TASKS)
