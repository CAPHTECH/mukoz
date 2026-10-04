# Task: hex_encode

void hex_encode(uint8_t *dst, const uint8_t *src, uint64_t n): write the lowercase hexadecimal form of src[0..n) to dst[0..2n): dst[2i] is the high nibble digit and dst[2i+1] the low nibble digit of src[i] ('0'-'9','a'-'f'). dst points to exactly 2n writable bytes, src to n readable bytes.

Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). The routine must not make system calls and must only touch the memory described below (plus its own stack below the return address).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
