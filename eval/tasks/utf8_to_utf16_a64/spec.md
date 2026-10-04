# Task: utf8_to_utf16 (AArch64)

uint64_t utf8_to_utf16(uint16_t *dst, const uint8_t *src, uint64_t n): if src[0..n) is well-formed UTF-8 (RFC 3629 / Unicode Table 3-7: no overlong forms, no surrogates U+D800-U+DFFF, nothing above U+10FFFF, no truncated or stray bytes), write its UTF-16LE encoding to dst (code points above U+FFFF as surrogate pairs) and return the number of 16-bit units written. Otherwise return 0xffffffffffffffff (dst contents are then unspecified). dst has room for n units (2n bytes) and src points to exactly n readable bytes (n may be 0). Only dst[0..n) may be written, and for valid input nothing beyond the returned number of units may be modified.

Target CPU: AArch64 (ARMv8-A, little-endian). Calling convention: AAPCS64. Arguments in x0-x7 (w0-w7 for 32-bit and narrower), return value in x0 (or w0). Return with RET to the address in x30. Preserve x19-x29 and sp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). Use only base integer A64 instructions (no SIMD/FP). The routine must not make system calls and must only touch the memory described below (plus its own stack below the incoming sp).

Deliverable: a raw AArch64 machine-code file (little-endian 32-bit instruction words) whose first instruction is the entry point.
