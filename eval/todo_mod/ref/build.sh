#!/bin/sh
# Build the reference modules (and mutated variants) as raw images at their fixed bases.
set -eu
cd "$(dirname "$0")"
OUT=${1:-../bin}
mkdir -p "$OUT"
CF="-Os -nostdlib -ffreestanding -fno-stack-protector -fcf-protection=none -fno-asynchronous-unwind-tables -fno-builtin -mno-red-zone"
T=$(mktemp -d)
one() { # name src base flags
  gcc $CF $4 -c $2.c -o $T/o.o
  printf 'ENTRY(%s)\nSECTIONS { . = %s; .text : { *(.text.start) *(.text .text.*) *(.rodata .rodata.*) } . = 0x10000000; .bss (NOLOAD) : { *(.bss .bss.* COMMON) } /DISCARD/ : { *(.data*) *(.comment) *(.note*) *(.eh_frame*) } }\n' "$5" "$3" > $T/l.ld
  ld -T $T/l.ld -o $T/o.elf $T/o.o
  objcopy -O binary -j .text $T/o.elf "$OUT/$1.bin"
  echo "$1 $(wc -c < "$OUT/$1.bin")"
}
i=1
for m in parse_id udec find_rec fmt_line make_rec del_rec clear_done; do
  one $m $m $(printf '0x%x' $((0x100000 + i * 0x100000))) "-fpie" $m
  i=$((i + 1))
done
one main main 0x100000 "-fno-pic -fno-pie" _start
one parse_id_mut_leadzero parse_id 0x200000 "-fpie -DMUT_PARSE_LEADZERO" parse_id
one udec_mut_nul udec 0x300000 "-fpie -DMUT_UDEC_NUL" udec
one find_rec_mut_16 find_rec 0x400000 "-fpie -DMUT_FIND_16" find_rec
one main_mut_deln main 0x100000 "-fno-pic -fno-pie -DMUT_MAIN_DELN" _start
rm -rf $T
