#!/bin/sh
set -eu
cd "$(dirname "$0")"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
mkdir -p bin
for s in *.s; do n=${s%.s}; as --64 -o "$tmp/$n.o" "$s"; objcopy -O binary -j .text "$tmp/$n.o" "bin/$n.bin"; done
ls bin | wc -l
