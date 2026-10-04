# Task: base64

void base64_encode(uint8_t *dst, const uint8_t *src, uint64_t n): write the standard base64 encoding (RFC 4648 alphabet A-Z a-z 0-9 + /, with '=' padding) of src[0..n) to dst. dst points to exactly 4*ceil(n/3) writable bytes, src to n readable bytes. No terminating NUL.

Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). The routine must not make system calls and must only touch the memory described below (plus its own stack below the return address).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
