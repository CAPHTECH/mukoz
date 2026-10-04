#!/bin/sh
# Assemble every fixtures/x86_64/*.s into a raw binary (.text only) and write
# fixtures/x86_64/manifest.txt with sha256 and toolchain version.
set -eu
cd "$(dirname "$0")/x86_64"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
: > manifest.txt
echo "# toolchain: $(as --version | head -1)" >> manifest.txt
for s in *.s; do
  n=${s%.s}
  as --64 -o "$tmp/$n.o" "$s"
  objcopy -O binary -j .text "$tmp/$n.o" "$n.bin"
  echo "$n.bin $(wc -c < "$n.bin") $(sha256sum "$n.bin" | cut -d' ' -f1)" >> manifest.txt
done
cat manifest.txt
