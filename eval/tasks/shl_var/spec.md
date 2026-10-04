# Task: shl_var

uint64_t shl_var(uint64_t x, uint64_t s): return x shifted left by s bits as a mathematical operation on 64-bit values: the result is 0 when s >= 64 (s is a full 64-bit value).

Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). The routine must not make system calls and must only touch the memory described below (plus its own stack below the return address).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
