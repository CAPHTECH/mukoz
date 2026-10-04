# Task: base64 (AArch64)

void base64_encode(uint8_t *dst, const uint8_t *src, uint64_t n): write the standard base64 encoding (RFC 4648 alphabet A-Z a-z 0-9 + /, with '=' padding) of src[0..n) to dst. dst points to exactly 4*ceil(n/3) writable bytes, src to n readable bytes. No terminating NUL.

Target CPU: AArch64 (ARMv8-A, little-endian). Calling convention: AAPCS64. Arguments in x0-x7 (w0-w7 for 32-bit and narrower), return value in x0 (or w0). Return with RET to the address in x30. Preserve x19-x29 and sp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). Use only base integer A64 instructions (no SIMD/FP). The routine must not make system calls and must only touch the memory described below (plus its own stack below the incoming sp).

Deliverable: a raw AArch64 machine-code file (little-endian 32-bit instruction words) whose first instruction is the entry point.
