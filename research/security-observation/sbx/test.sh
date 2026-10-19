#!/usr/bin/env bash
# WP0.7 acceptance for the SUT sandbox. Run as obs in ~/sbx on the VM, with ~/e0 next to it.
# usage: ./test.sh <out-dir>     then: python3 check_sbx.py <out-dir>
#   SBX_SECTIONS="3 4"   run only those numbered sections (default: all)
#   SBX_STRACE_OPTS=...  strace options before -o (default: the WP0.7 acceptance options)
set -u
SECTIONS=" ${SBX_SECTIONS:-1 2 3 4 5 6 7} "
STRACE_OPTS=${SBX_STRACE_OPTS:--f -yy -qq -s 0 -e trace=%file,%desc,%network,%process}
on() { [[ "$SECTIONS" == *" $1 "* ]]; }
OUT=$(realpath -m "${1:?out dir}")
HERE=$(cd "$(dirname "$0")" && pwd)
E0=$HERE/../e0
FX=/mnt/fx
mkdir -p "$OUT"

# Install root-owned copies outside /home (hidden inside the sandbox).
gcc -O2 -Wall -o "$HERE/sbxinit" "$HERE/sbxinit.c" || exit 1
( cd "$E0" && gcc -O2 -g -w -o cal cal.c && gcc -O0 -g -w -o cal-O0 cal.c && gcc -O2 -w -o fanmon fanmon.c ) || exit 1
sudo -n install -d -o root -g root -m 755 /opt/sbx /opt/e0
sudo -n install -o root -g root -m 755 "$HERE/sbx.sh" "$HERE/sbxinit" /opt/sbx/
sudo -n install -o root -g root -m 755 "$E0/cal" "$E0/cal-O0" "$HERE/probe.sh" /opt/e0/
SBX="sudo -n /opt/sbx/sbx.sh"

fixture() {
  sudo -n umount "$FX" 2>/dev/null
  sudo -n mkdir -p "$FX"
  sudo -n mount -t tmpfs -o "strictatime,size=64m,mode=1777" e0fx "$FX" || exit 1
  for f in secret decoy public rw; do head -c 4096 /dev/urandom >"$FX/$f"; done
  chmod 0444 "$FX/secret"; ln -s secret "$FX/link"; mkdir "$FX/out"
  ( cd "$FX" && sha256sum rw ) >"$1/rw.before.sha256"
  touch -a -d '2020-01-01 00:00:00' "$FX/secret" "$FX/decoy" "$FX/public" "$FX/rw"
}

receivers() {   # $1 = id, $2 = run dir. Allowed 10.77.0.1:8081, forbidden 10.77.0.3:9090.
  sudo -n ip netns exec "tn-$1" python3 -I -c '
import socket, sys, threading
def serve(host, port, log):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind((host, port)); s.listen()
    while True:
        c, a = s.accept(); data = c.recv(65536); c.close()
        with open(log, "a") as f: f.write(f"{host}:{port} from={a[0]} bytes={len(data)}\n")
for h, p in (("10.77.0.1", 8081), ("10.77.0.3", 9090)):
    threading.Thread(target=serve, args=(h, p, sys.argv[1]), daemon=True).start()
print("READY", flush=True)
threading.Event().wait()
' "$2/receiver.txt" >"$2/receiver.ready" 2>&1 &
  RCV=$!
  for _ in $(seq 50); do grep -q READY "$2/receiver.ready" 2>/dev/null && return 0; sleep 0.1; done
  echo "receivers not ready" >"$2/ERROR"
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
  echo "layers not ready" >"$d/ERROR"
}

layers_stop() {
  sleep 0.3
  sudo -n kill -INT "$(pgrep -P "$BT" bpftrace || echo "$BT")" 2>/dev/null
  sudo -n kill -TERM "$(pgrep -P "$FAN" fanmon || echo "$FAN")" 2>/dev/null
  wait "$FAN" "$BT" 2>/dev/null
}

# one <name> <grace-ms> <tracer: none|strace> -- <cmd...>
one() {
  local name=$1 grace=$2 tracer=$3; shift 3; [ "$1" = "--" ] && shift
  local d=$OUT/$name id=t$$
  mkdir -p "$d"
  fixture "$d"
  $SBX up "$id" >/dev/null || { echo "up failed" >"$d/ERROR"; return; }
  receivers "$id" "$d"
  layers_start "$d"
  printf '%q ' "$@" >"$d/cmd.txt"
  local pre=()
  # shellcheck disable=SC2206
  [ "$tracer" = strace ] && pre=(strace $STRACE_OPTS -o "$d/strace.txt")
  sudo -n env SBX_INJECT="${SBX_INJECT:-}" SBX_DIAG="${SBX_DIAG:-}" "${pre[@]}" /opt/sbx/sbx.sh run "$id" "$d" "$FX" /opt/e0 "$grace" -- "$@"
  echo $? >"$d/exit.txt"
  layers_stop
  $SBX stop "$id"
  $SBX down "$id" "$d" >/dev/null
  sudo -n kill "$(pgrep -P "$RCV" python3 || echo "$RCV")" 2>/dev/null; wait "$RCV" 2>/dev/null
  ( cd "$FX" && find . -xdev -printf '%i %P %y\n' ) >"$d/inodes.txt"
  ( cd "$FX" && sha256sum rw ) >"$d/rw.sha256"
  sudo -n chown -R obs:obs "$d"
  echo "done $name exit=$(cat "$d/exit.txt")"
}

# 1. What the SUT can see and do.
on 1 && one sbx-probe 1000 none -- /sut/probe.sh

# 2. Fault injection: a failed check must stop the SUT from starting.
on 2 && SBX_INJECT=blocked one sbx-inject 1000 none -- /sut/probe.sh

# 3. Calibration items through the sandbox, strace outside, both ground-truth layers.
on 3 && for it in noop fdreuse fail kxfer child symlink restore late normal; do
  one "sbx-strace-$it" 1000 strace -- /sut/cal "$it" /srv/fx
done

# 4. pid 1 policy: with no grace period the detached child is killed (as in E0 ns-strace-late).
on 4 && one sbx-grace0-late 0 strace -- /sut/cal late /srv/fx

# 5. Memcheck inside the sandbox; XML goes to a controller file through fd 4.
on 5 && SBX_DIAG=$OUT/sbx-vg-heapover/vg.xml one sbx-vg-heapover 1000 none -- \
  valgrind --tool=memcheck --xml=yes --xml-fd=4 /sut/cal-O0 heapover /srv/fx

# 6. Budget: more processes than pids.max (256).
on 6 && one sbx-pids 1000 none -- /bin/bash -c 'n=0; for i in $(seq 400); do sleep 2 & n=$((n+1)); done 2>/dev/null; wait; echo started=$n'

# 7. Leftovers: a child that outlives the grace period. sbxinit exits, the pid namespace is torn
#    down, and `down` confirms through the cgroup that nothing is left.
on 7 && one sbx-leftover 300 none -- /bin/bash -c 'setsid sleep 60 & exit 0'

sudo -n umount "$FX"
echo ALL-DONE
