# gcc-generated reference (experimenter only)

`b64.c` compiled with `gcc -O2 -fPIC -fno-asynchronous-unwind-tables -fcf-protection=none`,
linked with `link.ld` (text at 0, rodata after it) and flattened with `objcopy -O binary`
to `../bin/b64_gcc.bin` (384 bytes, table included).

`b64_gcc_mut_mask.bin`: one byte changed at offset 0xd4 (`and eax,0x3c` -> `and eax,0x38`,
third output character of the 2-byte tail). Mukoz: REJECT task.base64/char2. Oracle: FAIL.
