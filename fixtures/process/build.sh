#!/bin/sh
# Build the process fixtures. x86-64: GNU as / gcc (static, no libc, no PIE).
# AArch64: hand-encoded words wrapped into a minimal ELF by mkelf.py; Mach-O
# files by mkmacho.py (rustc aarch64 target + llvm-objcopy for the code). Writes manifest.txt with sizes and sha256.
set -eu
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
as --64 --defsym LEN=6 -o "$tmp/h.o" hello_x86.s && objcopy -O binary -j .text "$tmp/h.o" hello_x86.bin
as --64 --defsym LEN=5 -o "$tmp/h5.o" hello_x86.s && objcopy -O binary -j .text "$tmp/h5.o" hello_x86_mut_len.bin
ld -static -nostdlib --build-id=none -o hello_x86.elf "$tmp/h.o"
ld -static -nostdlib --build-id=none -o hello_x86_mut_len.elf "$tmp/h5.o"
as --64 -o "$tmp/heq.o" hello_x86_eq.s && ld -static -nostdlib --build-id=none -o hello_x86_eq.elf "$tmp/heq.o"
as --64 --defsym LOOPS=1 -o "$tmp/f1.o" fake_accept_x86.s && ld -static -nostdlib --build-id=none -o fake_accept_x86.elf "$tmp/f1.o"
as --64 --defsym LOOPS=0 -o "$tmp/f0.o" fake_accept_x86.s && ld -static -nostdlib --build-id=none -o fake_accept_x86_flood.elf "$tmp/f0.o"
gcc -O2 -Wl,--build-id=none -o hello_dyn.elf hello_dyn.c
CF="-O2 -static -nostdlib -fno-pie -no-pie -fno-stack-protector -fcf-protection=none -fno-asynchronous-unwind-tables -fno-builtin -Wl,--build-id=none"
gcc $CF -o todo_x86.elf todo.c
gcc $CF -DMUT_NONL -o todo_x86_mut_nonl.elf todo.c
gcc $CF -DMUT_NOAPPEND -o todo_x86_mut_noappend.elf todo.c
gcc $CF -DMUT_STAT -o todo_x86_mut_stat.elf todo.c
python3 mkelf.py
python3 mkbroken.py
python3 mkmacho.py
: > manifest.txt
echo "# toolchain: $(as --version | head -1); $(gcc --version | head -1)" >> manifest.txt
for f in *.bin *.elf *.macho; do echo "$f $(wc -c < "$f") $(sha256sum "$f" | cut -d' ' -f1)" >> manifest.txt; done
cat manifest.txt
