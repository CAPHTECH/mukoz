# Task: memmove

void memmove_(uint8_t *dst, const uint8_t *src, uint64_t n): copy n bytes from src to dst; the two ranges may overlap (both lie inside one buffer); the result must be as if the bytes were first copied to a temporary buffer. No return value.

Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). The routine must not make system calls and must only touch the memory described below (plus its own stack below the return address).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
