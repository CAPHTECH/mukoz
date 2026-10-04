# Task: codec

uint64_t codec(uint64_t op, uint8_t *dst, const uint8_t *src, uint64_t n) — a converter selected by op. src points to exactly n readable bytes (n may be 0); dst points to 2n+4 writable bytes. On success it writes the output to dst[0..k) and returns k as described below, and must not modify dst beyond the output. On invalid input, or if op is not 0-3, it returns 0xffffffffffffffff (dst contents are then unspecified, but only dst[0..2n+4) may be written).

- op 0, UTF-8 to UTF-16LE: src must be well-formed UTF-8 (RFC 3629 / Unicode Table 3-7: no overlong forms, no surrogates U+D800-U+DFFF, nothing above U+10FFFF, no truncated or stray bytes). Output: the UTF-16LE encoding (code points above U+FFFF as surrogate pairs). Returns the number of 16-bit units (the output has 2*k bytes in that case; "dst beyond the output" means beyond those 2*k bytes).
- op 1, UTF-16LE to UTF-8: n must be even and src must be well-formed UTF-16LE (every high surrogate D800-DBFF immediately followed by a low surrogate DC00-DFFF, no unpaired low surrogate). Output: the UTF-8 encoding. Returns the number of bytes.
- op 2, base64 encode (RFC 4648 alphabet A-Z a-z 0-9 + /, with = padding). Returns the number of bytes (4*ceil(n/3)).
- op 3, base64 decode, strict: n must be a multiple of 4; only alphabet characters, except that the last group may end in "=" or "=="; the unused bits before padding must be zero (canonical encoding). Output: the decoded bytes. Returns their number.

Calling convention: x86-64 System V. Arguments in rdi, rsi, rdx, rcx, r8, r9; return value in rax (or eax for 32-bit results). Preserve rbx, rbp, r12-r15 and rsp. Upper bits of registers that carry arguments narrower than 64 bits are unspecified (may be garbage). The routine must not make system calls and must only touch the memory described below (plus its own stack below the return address).

Deliverable: a raw x86-64 machine-code file whose first byte is the entry point.
