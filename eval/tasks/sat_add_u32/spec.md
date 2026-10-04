# Task: sat_add_u32

uint32_t sat_add_u32(uint32_t a, uint32_t b): return a + b as unsigned 32-bit, saturating at 0xffffffff instead of wrapping. Only the low 32 bits of the return register (eax) are checked.

Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). The routine must not make system calls and must only touch the memory described below (plus its own stack below the return address).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
