#!/usr/bin/env python3
"""Generate src/qualify_vectors.rs: known-answer instruction tests for engine qualification
(docs/02 2.5, docs/09 9.6). Code is assembled with GNU as (x86_64) and rustc global_asm
(aarch64); expected values come from the Python reference semantics below, never from
Unicorn or Mukoz's expression evaluator. Run from the repository root."""
import os, subprocess, tempfile, hashlib, glob

M = (1 << 64) - 1
M32 = (1 << 32) - 1
def s64(x): return x - (1 << 64) if x >> 63 else x
def s32(x): x &= M32; return x - (1 << 32) if x >> 31 else x
def sx(x, bits): x &= (1 << bits) - 1; return (x - (1 << bits) if x >> (bits - 1) else x) & M
def tdiv(a, b):
    q = abs(a) // abs(b)
    return -q if (a < 0) != (b < 0) else q
def rol(a, n): n &= 63; return a if n == 0 else ((a << n) | (a >> (64 - n))) & M
def ror(a, n): n &= 63; return a if n == 0 else ((a >> n) | (a << (64 - n))) & M
def le(x, n=8): return x.to_bytes(n, 'little')
def parity_even(x): return 1 if bin(x & 0xff).count('1') % 2 == 0 else 0

INPUTS = [
    (0, 0), (1, 1), (M, 1), (1 << 63, M), ((1 << 63) - 1, 2), (0x123456789abcdef0, 0x0fedcba987654321),
    (0xdeadbeef, 63), (5, 67), (M, M), (0x80, 0x7f), (0xffffffff, 0xffffffff), (3, 0x8000000000000001),
]

def stack_bytes(a, b):
    buf = bytearray(32); buf[8:16] = le(a); buf[16:24] = le(b)
    return buf

X86 = [
    ("add", "mov rax, rdi\nadd rax, rsi", lambda a, b: ((a + b) & M, None)),
    ("sub", "mov rax, rdi\nsub rax, rsi", lambda a, b: ((a - b) & M, None)),
    ("and_or_xor", "mov rax, rdi\nand rax, rsi\nmov rdx, rdi\nor rdx, rsi\nxor rdx, rax", lambda a, b: (a & b, (a | b) ^ (a & b))),
    ("imul2", "mov rax, rdi\nimul rax, rsi", lambda a, b: ((s64(a) * s64(b)) & M, None)),
    ("imul3", "imul rax, rdi, 0x12345", lambda a, b: ((s64(a) * 0x12345) & M, None)),
    ("mul", "mov rax, rdi\nmul rsi", lambda a, b: ((a * b) & M, (a * b) >> 64)),
    ("imul1", "mov rax, rdi\nimul rsi", lambda a, b: ((s64(a) * s64(b)) & M, ((s64(a) * s64(b)) >> 64) & M)),
    ("mul32", "mov eax, edi\nmul esi", lambda a, b: (((a & M32) * (b & M32)) & M32, ((a & M32) * (b & M32)) >> 32)),
    ("div", "mov rax, rdi\nxor edx, edx\ndiv rsi", lambda a, b: (a // b, a % b) if b else None),
    ("div128", "mov rax, rdi\nmov edx, 1\ndiv rsi", lambda a, b: ((((1 << 64) | a) // b), ((1 << 64) | a) % b) if b > 1 else None),
    ("idiv", "mov rax, rdi\ncqo\nidiv rsi", lambda a, b: None if b == 0 or (a == 1 << 63 and b == M) else (tdiv(s64(a), s64(b)) & M, (s64(a) - tdiv(s64(a), s64(b)) * s64(b)) & M)),
    ("shl", "mov rax, rdi\nmov rcx, rsi\nshl rax, cl", lambda a, b: ((a << (b & 63)) & M, None)),
    ("shr", "mov rax, rdi\nmov rcx, rsi\nshr rax, cl", lambda a, b: (a >> (b & 63), None)),
    ("sar", "mov rax, rdi\nmov rcx, rsi\nsar rax, cl", lambda a, b: ((s64(a) >> (b & 63)) & M, None)),
    ("rol", "mov rax, rdi\nmov rcx, rsi\nrol rax, cl", lambda a, b: (rol(a, b), None)),
    ("ror", "mov rax, rdi\nmov rcx, rsi\nror rax, cl", lambda a, b: (ror(a, b), None)),
    ("shl32", "mov eax, edi\nmov ecx, esi\nshl eax, cl", lambda a, b: (((a & M32) << (b & 31)) & M32, None)),
    ("sar32", "mov eax, edi\nmov ecx, esi\nsar eax, cl", lambda a, b: ((s32(a) >> (b & 31)) & M32, None)),
    ("add32", "mov eax, edi\nadd eax, esi", lambda a, b: ((a + b) & M32, None)),
    ("shld", "mov rax, rdi\nshld rax, rsi, 13", lambda a, b: (((a << 13) | (b >> 51)) & M, None)),
    ("shrd", "mov rax, rdi\nshrd rax, rsi, 13", lambda a, b: (((a >> 13) | (b << 51)) & M, None)),
    ("movsx", "movsx rax, dil\nmovsx rdx, si", lambda a, b: (sx(a, 8), sx(b, 16))),
    ("movsxd_movzx", "movsxd rax, edi\nmovzx edx, sil", lambda a, b: (sx(a, 32), b & 0xff)),
    ("bswap", "mov rax, rdi\nbswap rax", lambda a, b: (int.from_bytes(le(a), 'big'), None)),
    ("neg_not", "mov rax, rdi\nneg rax\nmov rdx, rsi\nnot rdx", lambda a, b: ((-a) & M, (~b) & M)),
    ("cmov_signed_min", "mov rax, rdi\ncmp rdi, rsi\ncmovg rax, rsi", lambda a, b: (b if s64(a) > s64(b) else a, None)),
    ("cmov_unsigned_min", "mov rax, rdi\ncmp rdi, rsi\ncmova rax, rsi", lambda a, b: (b if a > b else a, None)),
    ("setcc_cmp", "xor eax, eax\nxor edx, edx\ncmp rdi, rsi\nsetl al\nsetb dl\nsete cl\nshl ecx, 8\nand ecx, 0x100\nor eax, ecx", lambda a, b: ((1 if s64(a) < s64(b) else 0) | ((1 if a == b else 0) << 8), 1 if a < b else 0)),
    ("setcc_add", "xor eax, eax\nxor edx, edx\nmov rcx, rdi\nadd rcx, rsi\nseto al\nsetc dl\nsetp r8b\nsets r9b\nmovzx r8d, r8b\nmovzx r9d, r9b\nshl r8d, 8\nshl r9d, 16\nor eax, r8d\nor eax, r9d",
        lambda a, b: ((1 if not (-(1 << 63) <= s64(a) + s64(b) < (1 << 63)) else 0) | (parity_even((a + b) & M) << 8) | ((((a + b) & M) >> 63) << 16), 1 if a + b > M else 0)),
    ("adc", "mov rax, rdi\nadd rax, rsi\nmov edx, 0\nadc rdx, 0", lambda a, b: ((a + b) & M, 1 if a + b > M else 0)),
    ("sbb", "mov rax, rdi\nsub rax, rsi\nsbb rdx, rdx", lambda a, b: ((a - b) & M, M if a < b else 0)),
    ("lea", "lea rax, [rdi + rsi*8 + 0x1234]", lambda a, b: ((a + b * 8 + 0x1234) & M, None)),
    ("bts_bt", "mov rax, rdi\nbts rax, rsi\nxor edx, edx\nbt rdi, rsi\nsetc dl", lambda a, b: (a | (1 << (b & 63)), (a >> (b & 63)) & 1)),
    ("bsr_bsf", "bsr rax, rdi\nbsf rdx, rdi", lambda a, b: (a.bit_length() - 1, (a & -a).bit_length() - 1) if a else None),
    # The engine's default CPU model has no POPCNT: the emulator must stop with an invalid
    # instruction (never return a wrong value); the real CPU must compute the count.
    ("popcnt", "popcnt rax, rdi", lambda a, b: (bin(a).count('1'), None), "unsupported_emulated"),
    ("push_pop", "push rdi\npush rsi\npop rax\npop rdx", lambda a, b: (b, a)),
    ("xchg", "mov rax, rdi\nmov rdx, rsi\nxchg rax, rdx", lambda a, b: (b, a)),
    ("call_ret", "call 2f\nadd rax, rsi\nret\n2:\nmov rax, rdi", lambda a, b: ((a + b) & M, None)),
    ("loop_sum", "xor eax, eax\nmov rcx, rdi\nand rcx, 0xff\ntest rcx, rcx\njz 2f\n1:\nadd rax, rcx\ndec rcx\njnz 1b\n2:", lambda a, b: ((a & 0xff) * ((a & 0xff) + 1) // 2, None)),
    ("cqo", "mov rax, rdi\ncqo", lambda a, b: (a, M if a >> 63 else 0)),
    ("rcl", "stc\nmov rax, rdi\nrcl rax, 1\nclc\nmov rdx, rsi\nrcl rdx, 1", lambda a, b: (((a << 1) | 1) & M, (b << 1) & M)),
    ("stack_unaligned", "sub rsp, 32\nmov [rsp+8], rdi\nmov [rsp+16], rsi\nmov eax, dword ptr [rsp+12]\nmov rdx, qword ptr [rsp+13]\nadd rsp, 32",
        lambda a, b: (int.from_bytes(stack_bytes(a, b)[12:16], 'little'), int.from_bytes(stack_bytes(a, b)[13:21], 'little'))),
    ("movabs", "movabs rax, 0x0123456789abcdef\nxor rax, rdi", lambda a, b: (a ^ 0x0123456789abcdef, None)),
    ("rep_stosb", "sub rsp, 64\nmov rax, rsi\nmov ecx, 8\nmov rdx, rdi\nmov rdi, rsp\nrep stosb\nmov rax, [rsp]\nmov rdi, rdx\nadd rsp, 64", lambda a, b: (int.from_bytes(bytes([b & 0xff]) * 8, 'little'), a)),
    ("cmpxchg", "mov rax, rdi\nmov rcx, rsi\nmov rdx, rdi\ncmpxchg rdx, rcx\nmov rax, rdx\nsete dl\nmovzx edx, dl", lambda a, b: (b, 1)),
]

A64 = [
    ("add_sub", "add x2, x0, x1\nsub x1, x0, x1\nmov x0, x2", lambda a, b: ((a + b) & M, (a - b) & M)),
    ("logic", "and x2, x0, x1\norr x3, x0, x1\neor x1, x2, x3\nbic x0, x0, x1", lambda a, b: (a & ~((a & b) ^ (a | b)) & M, (a & b) ^ (a | b))),
    ("mul_umulh", "mul x2, x0, x1\numulh x1, x0, x1\nmov x0, x2", lambda a, b: ((a * b) & M, (a * b) >> 64)),
    ("smulh", "smulh x0, x0, x1", lambda a, b: (((s64(a) * s64(b)) >> 64) & M, None)),
    ("udiv", "udiv x0, x0, x1", lambda a, b: (a // b if b else 0, None)),
    ("sdiv", "sdiv x0, x0, x1", lambda a, b: (0 if b == 0 else ((1 << 63) if (a == 1 << 63 and b == M) else tdiv(s64(a), s64(b)) & M), None)),
    ("shifts", "lsl x2, x0, x1\nlsr x3, x0, x1\neor x0, x2, x3\nasr x1, x0, x1", lambda a, b: (((a << (b & 63)) & M) ^ (a >> (b & 63)), (s64(((a << (b & 63)) & M) ^ (a >> (b & 63))) >> (b & 63)) & M)),
    ("ror", "ror x0, x0, x1", lambda a, b: (ror(a, b), None)),
    ("add32", "add w0, w0, w1\nlsl w1, w1, #3", lambda a, b: ((a + b) & M32, (b << 3) & M32)),
    ("clz_rbit", "clz x2, x0\nrbit x1, x0\nmov x0, x2", lambda a, b: (64 - a.bit_length(), int(format(a, '064b')[::-1], 2))),
    ("rev_cls", "rev x2, x0\ncls x1, x1\nmov x0, x2", lambda a, b: (int.from_bytes(le(a), 'big'), next((i for i in range(63) if ((b >> (62 - i)) & 1) != (b >> 63)), 63))),
    ("csel_cset", "cmp x0, x1\ncsel x2, x0, x1, lt\ncset x1, lo\nmov x0, x2", lambda a, b: (a if s64(a) < s64(b) else b, 1 if a < b else 0)),
    ("adds_carry", "adds x2, x0, x1\ncset x1, cs\nmov x0, x2", lambda a, b: ((a + b) & M, 1 if a + b > M else 0)),
    ("subs_carry", "subs x2, x0, x1\ncset x1, cs\nmov x0, x2", lambda a, b: ((a - b) & M, 1 if a >= b else 0)),
    ("overflow_neg", "adds x2, x0, x1\ncset x0, vs\ncset x1, mi", lambda a, b: (1 if not (-(1 << 63) <= s64(a) + s64(b) < (1 << 63)) else 0, ((a + b) & M) >> 63)),
    ("adc", "adds x2, x0, x1\nadc x1, xzr, xzr\nmov x0, x2", lambda a, b: ((a + b) & M, 1 if a + b > M else 0)),
    ("madd_msub", "madd x2, x0, x1, x0\nmsub x1, x0, x1, x1\nmov x0, x2", lambda a, b: ((a * b + a) & M, (b - a * b) & M)),
    ("bitfield", "ubfx x2, x0, #8, #12\nsbfx x1, x1, #4, #8\nmov x0, x2", lambda a, b: ((a >> 8) & 0xfff, sx(b >> 4, 8))),
    ("extr", "extr x0, x0, x1, #13", lambda a, b: (((a << 51) | (b >> 13)) & M, None)),
    ("movk_sxtw", "movk x0, #0x1234, lsl #16\nsxtw x1, w1", lambda a, b: ((a & ~(0xffff << 16) & M) | (0x1234 << 16), sx(b, 32))),
    ("umull_smull", "umull x2, w0, w1\nsmull x1, w0, w1\nmov x0, x2", lambda a, b: ((a & M32) * (b & M32), (s32(a) * s32(b)) & M)),
    ("neg_mvn", "neg x0, x0\nmvn x1, x1", lambda a, b: ((-a) & M, (~b) & M)),
    ("stack_unaligned", "sub sp, sp, #32\nstp x0, x1, [sp, #8]\nldr w0, [sp, #12]\nldur x1, [sp, #13]\nadd sp, sp, #32",
        lambda a, b: (int.from_bytes(stack_bytes(a, b)[12:16], 'little'), int.from_bytes(stack_bytes(a, b)[13:21], 'little'))),
    ("call_ret", "stp x29, x30, [sp, #-16]!\nbl 2f\nadd x0, x0, x1\nldp x29, x30, [sp], #16\nret\n2:\nadd x0, x0, #1", lambda a, b: ((a + 1 + b) & M, None)),
    ("loop_sum", "and x2, x0, #0xff\nmov x0, #0\ncbz x2, 2f\n1:\nadd x0, x0, x2\nsubs x2, x2, #1\nb.ne 1b\n2:", lambda a, b: ((a & 0xff) * ((a & 0xff) + 1) // 2, None)),
    ("ccmp", "cmp x0, x1\nccmp x0, #5, #0, ne\ncset x0, eq", lambda a, b: (1 if (a != b and a == 5) else 0, None)),
    ("tbz", "tbz x0, #63, 2f\nmov x0, #1\nret\n2:\nmov x0, #0", lambda a, b: (a >> 63, None)),
]

T95 = glob.glob(os.path.expanduser('~/.rustup/toolchains/*/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objcopy'))[0]

def asm_x86(body):
    with tempfile.TemporaryDirectory() as d:
        open(f'{d}/t.s', 'w').write('.intel_syntax noprefix\n.text\n' + body + '\nret\n')
        subprocess.check_call(['as', '--64', '-o', f'{d}/t.o', f'{d}/t.s'])
        subprocess.check_call(['objcopy', '-O', 'binary', '-j', '.text', f'{d}/t.o', f'{d}/t.bin'])
        return open(f'{d}/t.bin', 'rb').read()

def asm_a64(body):
    lines = (body + '\nret').split('\n')
    with tempfile.TemporaryDirectory() as d:
        src = '#![no_std]\ncore::arch::global_asm!(' + ', '.join('"%s"' % l for l in lines) + ');\n'
        open(f'{d}/t.rs', 'w').write(src)
        subprocess.check_call(['rustc', '--target', 'aarch64-unknown-linux-gnu', '--crate-type=lib', '--emit=obj', '-C', 'panic=abort', '-o', f'{d}/t.o', f'{d}/t.rs'])
        subprocess.check_call([T95, '-O', 'binary', '-j', '.text', f'{d}/t.o', f'{d}/t.bin'])
        return open(f'{d}/t.bin', 'rb').read()

def emit(name, tests, asm):
    out = [f'pub const {name}: &[Test] = &[']
    n = 0
    for t in tests:
        tname, body, ref = t[:3]
        unsupported = len(t) > 3
        code = asm(body)
        vecs = []
        for a, b in INPUTS:
            r = ref(a, b)
            if r is None:
                continue
            o0, o1 = r
            vecs.append(f'({a:#x}, {b:#x}, {o0 & M:#x}, ' + (f'Some({o1 & M:#x})' if o1 is not None else 'None') + ')')
            n += 1
        out.append(f'    Test {{ name: "{tname}", emulated_unsupported: {str(unsupported).lower()}, code: &[{", ".join(hex(x) for x in code)}], vectors: &[{", ".join(vecs)}] }},')
    out.append('];')
    return '\n'.join(out), n

x, nx = emit('X86_64', X86, asm_x86)
a, na = emit('AARCH64', A64, asm_a64)
body = x + '\n\n' + a + '\n'
digest = hashlib.sha256(body.encode()).hexdigest()
open('src/qualify_vectors.rs', 'w').write(
    '//! Generated by tools/qualify/gen.py; do not edit. Expected values come from the reference\n'
    '//! semantics in that script, not from the engine or the expression evaluator.\n'
    'use crate::qualify::Test;\n\n'
    f'pub const TEST_SET: &str = "mukoz-qualify/1 {digest[:16]}";\n\n' + body)
print(f'x86_64: {len(X86)} tests, {nx} vectors; aarch64: {len(A64)} tests, {na} vectors; set {digest[:16]}')
