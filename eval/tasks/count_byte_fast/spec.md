# Task: count_byte_fast

uint64_t count_byte_fast(const uint8_t *buf, uint64_t n, uint8_t c): return how many of buf[0..n) equal c. buf points to exactly n readable bytes; c is passed in the low 8 bits of edx. Performance requirement: process the buffer 8 bytes per load (64-bit loads combined with word-at-a-time / SWAR bit tricks); a loop that handles every byte individually is not acceptable. At most 7 leftover bytes at the end may be handled one at a time. Unaligned 64-bit loads are allowed, but never read outside buf[0..n).

Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). The routine must not make system calls and must only touch the memory described below (plus its own stack below the return address).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
