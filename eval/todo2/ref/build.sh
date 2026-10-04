#!/bin/sh
# Reference and mutants as raw x86-64 process images (eval/todo2/spec.md). Static buffers live in
# .bss at 0x10000000 (the zero-filled data area); only .text is written to the image.
set -eu
cd "$(dirname "$0")"
mkdir -p ../bin
CF="-Os -fno-pic -fno-pie -nostdlib -ffreestanding -fno-stack-protector -fcf-protection=none -fno-asynchronous-unwind-tables -fno-builtin -mno-red-zone"
for v in "todo2:" "todo2_mut_stable:-DMUT_STABLE" "todo2_mut_case:-DMUT_CASE" "todo2_mut_importlast:-DMUT_IMPORTLAST" "todo2_mut_errorder:-DMUT_ERRORDER" "todo2_mut_emptyimport:-DMUT_EMPTYIMPORT"; do
  name=${v%%:*}; flag=${v#*:}
  gcc $CF $flag -c todo2.c -o /tmp/todo2_$$.o
  ld -T link.ld -o /tmp/todo2_$$.elf /tmp/todo2_$$.o
  test "$(objdump -h /tmp/todo2_$$.elf | awk '$2==".bss"{print $4}')" = "0000000010000000"
  objcopy -O binary -j .text /tmp/todo2_$$.elf ../bin/$name.bin
  echo "$name $(wc -c < ../bin/$name.bin)"
done
rm -f /tmp/todo2_$$.o /tmp/todo2_$$.elf
