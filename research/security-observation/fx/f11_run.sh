#!/usr/bin/env bash
# Run on the dedicated SEC-OBS VM, as obs, from ~/fx. Fresh output prefix required.
# This is calibration of implementation 1, public cases, one repetition; not E1.
set -euo pipefail
cd "$(dirname "$0")"
prefix=${1:-runs/f11-r1}
for mode in native a b c0 c; do
  if [ -e "$prefix-$mode" ]; then
    echo "Output exists: $prefix-$mode; choose a fresh prefix" >&2
    exit 2
  fi
done
mkdir -p build
for v in V R H; do
  suffix=$(echo "$v" | tr A-Z a-z)
  gcc -O2 -g -Wall -Wextra -DVARIANT_$v -o "build/f11-$suffix" src/f11.c
  sudo -n install -o root -g root -m 755 "build/f11-$suffix" /opt/fx/bin/
done
for mode in native a b c0 c; do
  FX_NOBUILD=1 FX_MODE=$mode ./run.sh "$prefix-$mode" F11
done
# f11_check uses the existing normalize/ and adapter/ alongside fx/.
python3 f11_check.py "$prefix"
