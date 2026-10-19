#!/usr/bin/env bash
# WP1 pilot runs (DESIGN.md 6, 7.2, 17): every artifact (scenario x V/R/H) x public case, as a
# resident SUT in the WP0.7 sandbox: start, wait for LISTEN, send the case's requests, SIGTERM the
# SUT's main process, wait for sbxinit. Ground truth is taken outside the SUT: fanotify and VFS
# bpftrace for files, the receivers and a SYN capture on the test network for connections, and a
# gdb hardware watchpoint on the artifact itself for memory (F10, F12; no strace there, both ptrace).
# Files and network scenarios also run under strace (B's trace for later; not ground truth).
# Run as obs on the VM in ~/fx with ~/sbx and ~/e0 next to it.
# usage: ./run.sh <out-dir> [F01 F03 ...]     then, on the workstation: python3 witness.py <out-dir>
#        FX_PLAN=<plan> ./run.sh <out-dir>    E1 (11.2): run the plan's lines "<sc> <variant> <case> <cond> <impl> <seed>"
#                         in order; cond a0|a1|b|c0|c, impl 1 (src/) or 2 (src2/), contracts from
#                         $FX_CONTRACTS/s<seed>/ (seed.py); each run goes to <out>/i<impl>-s<seed>-<cond>/<sc>/<v>/<case>
#   FX_NOBUILD=1      skip building and installing (a parallel worker after one build)
#   FX_MNT=<dir>      the fixture mount point (default /mnt/fx; one per parallel worker)
#   FX_ONLY=<regex>   run only the <scenario>/<variant>/<case> matching it (e.g. 'V/F0.-p05')
#   FX_MODE=native|a|b|c0|c native (default): the artifact as is, gdb witness for F10/F12.
#                         a: the artifact as is with no tracer (the A0/A1 condition, WP2); gdb witness for
#                         F10/F12 only in plan runs (E1, where it gives the block's memory ground truth).
#                         b: under strace for every scenario, no gdb (E1).
#                         c0: under Valgrind Nulgrind, c: under Memcheck (WP4). Both always under strace,
#                         no gdb; Valgrind's output goes to the controller through fd 4 (vg.log / vg.xml).
#   FX_ATIME=old|recent   fixture atime: 2020 (default), or a read just before the case (relatime then skips it)
#   FX_MOUNT=strictatime|relatime   fixture mount atime option (default strictatime; 12.2 auxiliary: relatime)
# Every run records the fixture manifest (A1, 9.1) before the SUT starts and after it has stopped.
set -u
OUT=$(realpath -m "${1:?out dir}"); shift
SCEN=${*:-F01 F03 F05 F08 F10 F12}
HERE=$(cd "$(dirname "$0")" && pwd)
E0=$HERE/../e0
FX=${FX_MNT:-/mnt/fx}
STRACE_OPTS="-f -yy -q --decode-pids=pidns -s 0 -e trace=%file,%desc,%network,%process"
GRACE=1000
setmode() {
  MODE=$1
  case $MODE in
    native|a|b) VG=() ;;
    c0) VG=(valgrind --tool=none --trace-children=no --log-fd=4) ;;
    c) VG=(valgrind --tool=memcheck --xml=yes --xml-fd=4 --error-limit=no --trace-children=no --leak-check=no) ;;
    *) echo "FX_MODE: native, a, b, c0 or c" >&2; exit 2 ;;
  esac
}
setmode "${FX_MODE:-native}"
RUSTC=$HOME/.cargo/bin/rustc
mkdir -p "$OUT" "$HERE/build" "$HERE/build2"
IMPL=1 RUNOUT=$OUT CDIR=$HERE/contract

# Build every artifact and install root-owned copies outside /home (hidden in the sandbox).
[ -n "${FX_NOBUILD:-}" ] || {
for v in v r h; do
  V=$(echo $v | tr a-z A-Z)
  for f in f01 f03 f05 f08; do
    "$RUSTC" -O --edition 2021 --cfg "variant=\"$v\"" --check-cfg 'cfg(variant, values("v","r","h"))' \
      -o "$HERE/build/$f-$v" "$HERE/src/$f.rs" || exit 1
  done
  for f in f10 f11 f12; do gcc -O2 -g -Wall -DVARIANT_$V -o "$HERE/build/$f-$v" "$HERE/src/$f.c" || exit 1; done
  for f in f01 f03 f05 f08 f10 f12; do gcc -O2 -g -Wall -DVARIANT_$V -o "$HERE/build2/$f-$v" "$HERE/src2/$f.c" || exit 1; done
done
gcc -O2 -Wall -o "$HERE/../sbx/sbxinit" "$HERE/../sbx/sbxinit.c" || exit 1
( cd "$E0" && gcc -O2 -w -o fanmon fanmon.c ) || exit 1
sudo -n install -d -o root -g root -m 755 /opt/sbx /opt/fx/bin /opt/fx/bin2
sudo -n install -o root -g root -m 755 "$HERE/../sbx/sbx.sh" "$HERE/../sbx/sbxinit" /opt/sbx/
sudo -n install -o root -g root -m 755 "$HERE"/build/* /opt/fx/bin/
sudo -n install -o root -g root -m 755 "$HERE"/build2/* /opt/fx/bin2/
}
{ "$RUSTC" -V; gcc --version | head -1; strace -V | head -1; gdb --version | head -1; valgrind --version; uname -r
  ( cd /opt/fx && sha256sum bin/* bin2/* )
  # A canary in a response implies the SUT read it only if no artifact carries the canary itself.
  echo "artifacts containing a canary: $(grep -lE 'CANARY-|TOKEN-F12' /opt/fx/bin/* /opt/fx/bin2/* | wc -l)"; } >"$OUT/env.txt"
SBX="sudo -n /opt/sbx/sbx.sh"

fixture() {   # $1 = contract
  sudo -n umount "$FX" 2>/dev/null
  sudo -n mkdir -p "$FX"
  sudo -n mount -t tmpfs -o "${FX_MOUNT:-strictatime},size=64m,mode=1777" fxfx "$FX" || exit 1
  python3 -I "$HERE/mkfx.py" "$1" "$FX" "${FX_ATIME:-old}" || exit 1
}

# Same receivers and layers as sbx/test.sh. Allowed 10.77.0.1:8081, forbidden 10.77.0.3:9090.
receivers() {
  sudo -n ip netns exec "tn-$1" python3 -I -c '
import socket, sys, threading
def serve(host, port, log):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind((host, port)); s.listen()
    while True:
        c, a = s.accept(); data = b""
        c.settimeout(2)
        try:
            while chunk := c.recv(65536): data += chunk
        except OSError: pass
        c.close()
        with open(log, "a") as f: f.write(f"{host}:{port} from={a[0]} bytes={len(data)}\n")
for h, p in (("10.77.0.1", 8081), ("10.77.0.3", 9090)):
    threading.Thread(target=serve, args=(h, p, sys.argv[1]), daemon=True).start()
print("READY", flush=True)
threading.Event().wait()
' "$2/receiver.txt" >"$2/receiver.ready" 2>&1 &
  RCV=$!
  for _ in $(seq 50); do grep -q READY "$2/receiver.ready" 2>/dev/null && return 0; sleep 0.1; done
  echo "receivers not ready" >>"$2/ERROR"
}

syncap() {   # SYNs the SUT sends onto the test network (connection attempts), outside the SUT.
  sudo -n ip netns exec "tn-$1" tcpdump -i "vt-$1" -nn -l -U \
    'src host 10.77.0.2 and tcp[tcpflags] & (tcp-syn|tcp-ack) == tcp-syn' >"$2/syn.txt" 2>"$2/syn.err" &
  SYN=$!
  for _ in $(seq 50); do grep -q 'listening on' "$2/syn.err" 2>/dev/null && return 0; sleep 0.1; done
  echo "syn capture not ready" >>"$2/ERROR"
}

layers_start() {
  local d=$1 dev; dev=$(stat -c %d "$FX")
  sudo -n "$E0/fanmon" "$FX" "$d/fan.txt" >"$d/fan.ready" 2>"$d/fan.err" &
  FAN=$!
  sudo -n bpftrace -q "$E0/vfs.bt" "$dev" >"$d/vfs.txt" 2>"$d/vfs.err" &
  BT=$!
  for _ in $(seq 100); do
    grep -q READY "$d/fan.ready" 2>/dev/null && grep -q READY "$d/vfs.txt" 2>/dev/null && return 0
    sleep 0.1
  done
  echo "layers not ready" >>"$d/ERROR"
}

layers_stop() {
  sleep 0.3
  sudo -n kill -INT "$(pgrep -P "$BT" bpftrace || echo "$BT")" 2>/dev/null
  sudo -n kill -TERM "$(pgrep -P "$FAN" fanmon || echo "$FAN")" 2>/dev/null
  sudo -n kill -INT "$(pgrep -P "$SYN" tcpdump || echo "$SYN")" 2>/dev/null
  wait "$FAN" "$BT" "$SYN" 2>/dev/null
}

# one <scenario> <variant> <case-id>
one() {
  local sc=$1 v=$2 cs=$3 d=$RUNOUT/$1/$2/$3 id=f$$ contract=$CDIR/$1.json
  local bin; bin=$(echo "$sc" | tr F f)-$(echo "$v" | tr A-Z a-z)
  local bindir=/opt/fx/bin gdbdir=$HERE/gdb
  [ "$IMPL" = 2 ] && bindir=/opt/fx/bin2 gdbdir=$HERE/gdb2
  [ -e "$d" ] && { echo "exists: $d" >&2; return; }
  mkdir -p "$d"
  echo "t_start $(date +%s.%N)" >"$d/timing.txt"
  fixture "$contract"
  $SBX up "$id" >/dev/null || { echo "up failed" >>"$d/ERROR"; return; }
  receivers "$id" "$d"
  syncap "$id" "$d"
  layers_start "$d"
  python3 -I "$HERE/manifest.py" "$FX" "$d/manifest.before.json"
  local pre=() witness=
  { [ "$MODE" = native ] || { [ "$MODE" = a ] && [ -n "${FX_PLAN:-}" ]; }; } && [ -e "$gdbdir/$sc.gdb" ] && witness=gdb
  # shellcheck disable=SC2206
  [ -z "$witness" ] && [ "$MODE" != a ] && pre=(strace $STRACE_OPTS -o "$d/strace.txt")
  local diag=
  [ "$MODE" = c0 ] && diag=$d/vg.log
  [ "$MODE" = c ] && diag=$d/vg.xml
  echo "${VG[*]} /sut/$bin /srv/fx" >"$d/cmd.txt"
  echo "t_sut $(date +%s.%N)" >>"$d/timing.txt"
  sudo -n env SBX_DIAG="$diag" "${pre[@]}" /opt/sbx/sbx.sh run "$id" "$d" "$FX" "$bindir" "$GRACE" -- "${VG[@]}" "/sut/$bin" /srv/fx &
  local run=$!
  local up=0
  for _ in $(seq 300); do
    sudo -n ip netns exec "sbx-$id" ss -ltnH 'sport = :8000' 2>/dev/null | grep -q LISTEN && { up=1; break; }
    sleep 0.1
  done
  [ $up = 1 ] || echo "SUT did not listen" >>"$d/ERROR"
  echo "t_listen $(date +%s.%N)" >>"$d/timing.txt"
  local gdbp=
  if [ -n "$witness" ] && [ $up = 1 ]; then
    sudo -n gdb -nx -batch -p "$($SBX pid "$id" "$d")" -x "$gdbdir/$sc.gdb" "$bindir/$bin" >"$d/gdb.txt" 2>&1 &
    gdbp=$!
    for _ in $(seq 100); do grep -q WATCHING "$d/gdb.txt" 2>/dev/null && break; sleep 0.1; done
    grep -q WATCHING "$d/gdb.txt" || echo "gdb not watching" >>"$d/ERROR"
  fi
  [ $up = 1 ] && { sudo -n ip netns exec "tn-$id" python3 -I "$HERE/client.py" "$contract" "$cs" "$d/responses.json" \
    || echo "client failed" >>"$d/ERROR"; }
  echo "t_client $(date +%s.%N)" >>"$d/timing.txt"
  sleep 0.2
  $SBX term "$id" "$d" 2>>"$d/ERROR"
  # $run is root's sudo, so kill -0 cannot probe it; a watchdog stops the case after 20 s instead.
  ( sleep 20; echo "SUT did not stop after SIGTERM" >>"$d/ERROR"; $SBX stop "$id" ) &
  local wd=$!
  wait $run; echo $? >"$d/exit.txt"
  kill $wd 2>/dev/null; wait $wd 2>/dev/null
  [ -n "$gdbp" ] && wait "$gdbp"
  echo "t_stopped $(date +%s.%N)" >>"$d/timing.txt"
  layers_stop
  python3 -I "$HERE/manifest.py" "$FX" "$d/manifest.after.json" --hash
  $SBX stop "$id"
  $SBX down "$id" "$d" >/dev/null
  sudo -n kill "$(pgrep -P "$RCV" python3 || echo "$RCV")" 2>/dev/null; wait "$RCV" 2>/dev/null
  ( cd "$FX" && find . -xdev -printf '%i %P %y\n' ) >"$d/inodes.txt"
  echo "t_end $(date +%s.%N)" >>"$d/timing.txt"
  sudo -n chown -R obs:obs "$d"
  echo "done $sc/$v/$cs exit=$(cat "$d/exit.txt")$([ -s "$d/ERROR" ] && echo " ERROR: $(tr '\n' ';' <"$d/ERROR")")"
}

if [ -n "${FX_PLAN:-}" ]; then
  while read -r sc v cs cond IMPL seed; do
    [ -z "$sc" ] && continue
    case $cond in a0|a1) setmode a ;; *) setmode "$cond" ;; esac
    RUNOUT=$OUT/i$IMPL-s$seed-$cond CDIR=${FX_CONTRACTS:?FX_CONTRACTS}/s$seed
    one "$sc" "$v" "$cs" </dev/null
  done <"$FX_PLAN"
  sudo -n umount "$FX"
  echo ALL-DONE
  exit 0
fi
for sc in $SCEN; do
  for v in V R H; do
    for cs in $(python3 -I -c 'import json,sys; print(" ".join(c["id"] for c in json.load(open(sys.argv[1]))["cases"]))' "$HERE/contract/$sc.json"); do
      [[ "$sc/$v/$cs" =~ ${FX_ONLY:-.} ]] && one "$sc" "$v" "$cs"
    done
  done
done
sudo -n umount "$FX"
echo ALL-DONE
