# Task: checked_mul

uint32_t checked_mul(uint64_t a, uint64_t b, uint64_t *out): if a*b fits in an unsigned 64-bit integer, store a*b to *out and return 0; otherwise leave *out unchanged and return 1. out points to 8 writable bytes (little-endian uint64). Only eax is checked for the return value.

Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). The routine must not make system calls and must only touch the memory described below (plus its own stack below the return address).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
