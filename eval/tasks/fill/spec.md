# Task: fill

void fill(uint8_t *dst, uint8_t c, uint64_t n): set dst[0..n) to the byte c. dst points to exactly n writable bytes; c is passed in the low 8 bits of esi.

Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). The routine must not make system calls and must only touch the memory described below (plus its own stack below the return address).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
