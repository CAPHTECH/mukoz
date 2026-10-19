#!/usr/bin/env bash
# E0 observer calibration on the dedicated VM (DESIGN.md 11.1). Run as obs inside ~/e0.
# usage: ./run.sh <out-dir>
# Each run gets a fresh tmpfs fixture, starts the ground-truth layers (fanmon, vfs.bt)
# as root, runs the subject as obs under one condition, stops the layers, then records
# inode map and file hashes. Nothing here reads the fixture while the layers are running.
set -u
OUT=${1:?out dir}
FX=/mnt/fx
mkdir -p "$OUT"
cd "$(dirname "$0")"
# cal at -O2 lost the heap errors to optimisation in run 1 (malloc/free elided), so the
# memory items also run at -O0. Both binaries are rebuilt here so env.txt digests match.
gcc -O2 -g -w -o cal cal.c && gcc -O0 -g -w -o cal-O0 cal.c && gcc -O2 -w -o fanmon fanmon.c || exit 1

fixture() {   # $1 = atime mode, $2 = run dir
  sudo -n umount "$FX" 2>/dev/null
  sudo -n mkdir -p "$FX"
  sudo -n mount -t tmpfs -o "$1,size=64m,mode=1777" e0fx "$FX" || exit 1
  head -c 4096 /dev/urandom >"$FX/secret"
  head -c 4096 /dev/urandom >"$FX/decoy"
  head -c 4096 /dev/urandom >"$FX/public"
  head -c 4096 /dev/urandom >"$FX/rw"
  chmod 0444 "$FX/secret"
  ln -s secret "$FX/link"
  mkdir "$FX/out"
  # Hash before resetting atime: reading rw here would otherwise move its atime (run 1 did).
  ( cd "$FX" && sha256sum rw ) >"$2/rw.before.sha256"
  touch -a -d '2020-01-01 00:00:00' "$FX/secret" "$FX/decoy" "$FX/public" "$FX/rw"
}

start_layers() {   # $1 = run dir
  local d=$1 dev
  dev=$(stat -c %d "$FX")
  sudo -n ./fanmon "$FX" "$d/fan.txt" >"$d/fan.ready" 2>"$d/fan.err" &
  FAN=$!
  sudo -n bpftrace -q vfs.bt "$dev" >"$d/vfs.txt" 2>"$d/vfs.err" &
  BT=$!
  for _ in $(seq 100); do
    grep -q READY "$d/fan.ready" 2>/dev/null && grep -q READY "$d/vfs.txt" 2>/dev/null && return 0
    sleep 0.1
  done
  echo "layers not ready" >"$d/ERROR"
  return 1
}

stop_layers() {
  sleep 0.5
  sudo -n kill -INT "$(pgrep -P "$BT" bpftrace || echo "$BT")" 2>/dev/null
  sudo -n kill -TERM "$(pgrep -P "$FAN" fanmon || echo "$FAN")" 2>/dev/null
  wait "$FAN" "$BT" 2>/dev/null
}

record() {   # $1 = run dir
  local d=$1
  # stat/ls -i do not open the files, so they raise no fanotify open/access events.
  ( cd "$FX" && find . -xdev -printf '%i %P %y\n' ) >"$d/inodes.txt"
  stat -c '%n %X' "$FX/secret" "$FX/decoy" "$FX/public" "$FX/rw" >"$d/atime.txt"
  ( cd "$FX" && sha256sum rw ) >"$d/rw.sha256"
}

one() {   # $1 = name, $2 = atime mode, rest = command
  local name=$1 mode=$2; shift 2
  local d="$OUT/$name"
  mkdir -p "$d"
  fixture "$mode" "$d"
  start_layers "$d" || { stop_layers; return; }
  printf '%q ' "$@" >"$d/cmd.txt"
  local t0 t1
  t0=$(date +%s.%N)
  "$@" >"$d/stdout.txt" 2>"$d/stderr.txt"
  echo $? >"$d/exit.txt"
  t1=$(date +%s.%N)
  echo "$t0 $t1" >"$d/time.txt"
  stop_layers
  record "$d"
  echo "done $name exit=$(cat "$d/exit.txt")"
}

ST="strace -f -yy -qq -s 0 -e trace=%file,%desc,%network,%process"
ITEMS="noop fdreuse fail kxfer child symlink restore late normal"

# B: strace on each calibration item (with both ground-truth layers).
for it in $ITEMS; do
  one "strace-$it" strictatime $ST -o "$OUT/strace-$it/strace.txt" ./cal "$it" "$FX"
done

# Ground-truth layers alone (no collector), to see the layers without strace present.
for it in $ITEMS; do
  one "bare-$it" strictatime ./cal "$it" "$FX"
done

# C0 / C: Nulgrind and Memcheck. Diagnostics go to controller files, not to the subject's stderr.
for bin in cal cal-O0; do
  for it in noop normal heapover uaf logical kxfer; do
    for tool in none memcheck; do
      n="vg-$tool-$bin-$it"
      one "$n" strictatime valgrind --tool=$tool --log-file="$OUT/$n/vg.log" \
        $( [ $tool = memcheck ] && echo --xml=yes --xml-file="$OUT/$n/vg.xml" ) ./$bin "$it" "$FX"
    done
  done
done

# B + C: strace outside, Memcheck inside (the combination used by condition C).
for it in fdreuse kxfer heapover; do
  one "strace-vg-$it" strictatime $ST -o "$OUT/strace-vg-$it/strace.txt" \
    valgrind --tool=memcheck --log-file="$OUT/strace-vg-$it/vg.log" ./cal-O0 "$it" "$FX"
done

# Tracing across user/pid/net namespaces (DESIGN.md 8.2 [U]).
for it in fdreuse child late; do
  one "ns-strace-$it" strictatime $ST -o "$OUT/ns-strace-$it/strace.txt" \
    unshare -Urn --pid --fork --kill-child ./cal "$it" "$FX"
done
one "ns-vg-heapover" strictatime unshare -Urn --pid --fork --kill-child \
  valgrind --tool=memcheck --log-file="$OUT/ns-vg-heapover/vg.log" ./cal-O0 heapover "$FX"

# atime: the same allowed read under strictatime and relatime.
one "atime-strict-normal" strictatime ./cal normal "$FX"
one "atime-relatime-normal" relatime ./cal normal "$FX"

sudo -n umount "$FX"
{ uname -r; strace -V | head -1; valgrind --version; bpftrace --version; gcc --version | head -1; sha256sum cal cal-O0 cal.c fanmon fanmon.c vfs.bt run.sh; } >"$OUT/env.txt" 2>&1
echo ALL-DONE
