#!/bin/sh
# Reference and mutants as raw x86-64 process images (eval/todo/spec.md).
set -eu
cd "$(dirname "$0")"
mkdir -p ../bin
CF="-Os -fno-pic -fno-pie -nostdlib -ffreestanding -fno-stack-protector -fcf-protection=none -fno-asynchronous-unwind-tables -fno-builtin -mno-red-zone"
for v in "todo:" "todo_mut_lastid:-DMUT_LASTID" "todo_mut_full49:-DMUT_FULL49" "todo_mut_zeropad:-DMUT_ZEROPAD" "todo_mut_leadzero:-DMUT_LEADZERO" "todo_mut_clearcreate:-DMUT_CLEARCREATE"; do
  name=${v%%:*}; flag=${v#*:}
  gcc $CF $flag -c todo.c -o /tmp/todo_$$.o
  ld -T link.ld -o /tmp/todo_$$.elf /tmp/todo_$$.o
  objcopy -O binary -j .text /tmp/todo_$$.elf ../bin/$name.bin
  echo "$name $(wc -c < ../bin/$name.bin) $(sha256sum ../bin/$name.bin | cut -c1-16)"
done
rm -f /tmp/todo_$$.o /tmp/todo_$$.elf
