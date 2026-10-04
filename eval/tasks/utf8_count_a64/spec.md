# Task: utf8_count (AArch64)

uint64_t utf8_count(const uint8_t *buf, uint64_t n): if buf[0..n) is well-formed UTF-8 (RFC 3629 / Unicode Table 3-7: no overlong forms, no surrogates U+D800-U+DFFF, nothing above U+10FFFF, no truncated or stray bytes), return the number of code points; otherwise return 0xffffffffffffffff. buf points to exactly n readable bytes (n may be 0).

Target CPU: AArch64 (ARMv8-A, little-endian). Calling convention: AAPCS64. Arguments in x0-x7 (w0-w7 for 32-bit and narrower), return value in x0 (or w0). Return with RET to the address in x30. Preserve x19-x29 and sp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). Use only base integer A64 instructions (no SIMD/FP). The routine must not make system calls and must only touch the memory described below (plus its own stack below the incoming sp).

Deliverable: a raw AArch64 machine-code file (little-endian 32-bit instruction words) whose first instruction is the entry point.
