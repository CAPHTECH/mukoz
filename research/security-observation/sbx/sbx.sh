#!/usr/bin/env bash
# SUT sandbox for SEC-OBS-01 (WP0.7, DESIGN.md 8.2 / 8.4). Run as root on the dedicated VM.
#
#   sbx.sh up   <id>                        cgroup + SUT netns + test-network netns (tn-<id>)
#   sbx.sh run  <id> <out-dir> <fixture> <bin-dir> <grace-ms> -- <cmd> [args...]
#   sbx.sh pid  <id> <out-dir>              outer pid of the SUT's main process (sbxinit's main_start)
#   sbx.sh term <id> <out-dir>              SIGTERM the SUT's main process only (a resident SUT's stop)
#   sbx.sh stop <id>                        SIGTERM everything in the case cgroup, then cgroup.kill
#   sbx.sh down <id> <out-dir>              record leftovers and budgets, remove cgroup and netns
#
# Inside the sandbox the SUT sees: its fixture at /srv/fx (same superblock as <fixture>, so the
# ground-truth layers outside see the same inodes), <bin-dir> read-only at /sut, fresh /tmp,
# and no /home, /root, /mnt. It runs as obs, no capabilities, no_new_privs, under sbxinit as pid 1.
# Network: 10.77.0.2/24 on a veth to tn-<id> (10.77.0.1 allowed receiver, 10.77.0.3 forbidden
# receiver); no other route. Any failed setup step or check exits 3 and the SUT is not started.
# Install under /opt/sbx (root-owned): /home is hidden inside the sandbox.
# SBX_DIAG=<file> opens <file> as fd 4 for an instrumenter such as valgrind --xml-fd=4.
set -u
SBX_DIR=$(cd "$(dirname "$0")" && pwd)
CG_ROOT=/sys/fs/cgroup/sec-obs
SUT_USER=obs
BLOCKED=3

# fd 3 is the status file once `run` has opened it (inherited by stage2).
die() { echo "BLOCKED $*" >&2; { echo "BLOCKED $*" >&3; } 2>/dev/null; exit $BLOCKED; }

cmd_up() {
  local id=$1 cg=$CG_ROOT/case-$1
  mkdir -p "$CG_ROOT" || die "cgroup root"
  echo "+cpu +memory +pids" >"$CG_ROOT/cgroup.subtree_control" 2>/dev/null
  [ -d "$cg" ] && die "case $id already up"
  mkdir "$cg" || die "cgroup $cg"
  echo 256 >"$cg/pids.max" && echo 1G >"$cg/memory.max" \
    && echo "200000 100000" >"$cg/cpu.max" || die "cgroup limits"
  [ -e "$cg/memory.swap.max" ] && echo 0 >"$cg/memory.swap.max"
  ip netns add "sbx-$id" && ip netns add "tn-$id" || die "netns add"
  ip link add "vs-$id" type veth peer name "vt-$id" || die "veth"
  ip link set "vs-$id" netns "sbx-$id" && ip link set "vt-$id" netns "tn-$id" || die "veth move"
  ip -n "sbx-$id" link set lo up && ip -n "sbx-$id" addr add 10.77.0.2/24 dev "vs-$id" \
    && ip -n "sbx-$id" link set "vs-$id" up || die "sut net"
  ip -n "tn-$id" link set lo up && ip -n "tn-$id" addr add 10.77.0.1/24 dev "vt-$id" \
    && ip -n "tn-$id" addr add 10.77.0.3/24 dev "vt-$id" && ip -n "tn-$id" link set "vt-$id" up || die "tn net"
  echo "up $id"
}

cmd_run() {
  local id=$1 out=$2 fx=$3 bin=$4 grace=$5; shift 5
  [ "${1:-}" = "--" ] && shift
  local cg=$CG_ROOT/case-$id
  mkdir -p "$out"
  exec 3>"$out/sbx.status"
  [ -d "$cg" ] || die "case $id is not up"
  ip netns list | grep -qw "sbx-$id" || die "netns sbx-$id missing"
  mountpoint -q "$fx" || die "fixture $fx is not a mount"
  # The supervisor itself joins the case cgroup so that everything it starts is counted there.
  echo $$ >"$cg/cgroup.procs" || die "join cgroup"
  if [ -n "${SBX_DIAG:-}" ]; then exec 4>"$SBX_DIAG" || die "diag fd"; fi
  exec ip netns exec "sbx-$id" unshare --mount --pid --ipc --uts --fork --propagation private \
    "$SBX_DIR/sbx.sh" stage2 "$id" "$fx" "$bin" "$grace" "$@" \
    </dev/null >"$out/stdout.txt" 2>"$out/stderr.txt"
}

# Runs as root, pid 1 of the new pid namespace, in the new mount namespace and sbx-<id> netns.
cmd_stage2() {
  local id=$1 fx=$2 bin=$3 grace=$4; shift 4
  mount -t proc proc /proc || die "mount proc"
  mkdir -p /srv/fx /sut || die "mkdir"
  mount --bind "$fx" /srv/fx || die "bind fixture"
  mount --bind "$bin" /sut && mount -o remount,bind,ro /sut || die "bind sut ro"
  local d
  # Everything the SUT may write is per-case. Run 1 of the acceptance test found /dev/mqueue,
  # /run/screen and /run/cloud-init/tmp writable and shared with the host.
  for d in /home /root /mnt /media /tmp /var/tmp /dev/shm /run; do
    [ -d "$d" ] || continue
    mount -t tmpfs -o size=16m,mode=1777,nosuid,nodev "sbx-$id" "$d" || die "hide $d"
  done
  mount -t mqueue mqueue /dev/mqueue || die "mqueue"
  hostname "sbx-$id" || die "hostname"
  # Checks. Each one must hold, or the SUT is not started.
  [ -e /home/obs/e0 ] && die "check: controller area visible"
  [ -e /opt/e0/runs ] && die "check: run records visible"
  [ -e /home/obs/.ssh ] && die "check: ssh keys visible"
  [ "$(ip route show default | wc -l)" = 0 ] || die "check: default route present"
  [ "$(ip -o route show | grep -cv '^10\.77\.0\.0/24 ')" = 0 ] || die "check: route other than 10.77.0.0/24"
  # /proc must be the new pid namespace's: pid 1 is this script, and no pid outside the case exists.
  tr '\0' ' ' </proc/1/cmdline | grep -q "sbx.sh stage2 $id " || die "check: /proc is not the case pid namespace"
  grep -q "^0::/sec-obs/case-$id\$" /proc/self/cgroup || die "check: not in case cgroup"
  local fo; fo=$(findmnt -no OPTIONS /srv/fx)
  echo "fixture_options $fo" >&3
  # The inherited cwd still points into the hidden /home (acceptance run s4); leave it.
  cd / || die "cd /"
  [ "$(pwd -P)" = / ] || die "check: cwd"
  [ "${SBX_INJECT:-}" = blocked ] && die "check: injected failure"
  echo "checks_passed t=$(date +%s%3N)" >&3
  exec setpriv --reuid="$SUT_USER" --regid="$SUT_USER" --clear-groups --no-new-privs \
    --inh-caps=-all --bounding-set=-all \
    prlimit --nofile=1024 --fsize=67108864 --core=0 -- \
    "$SBX_DIR/sbxinit" "$grace" "$@"
}

cmd_pid() {
  local cg=$CG_ROOT/case-$1 p inner
  inner=$(sed -n 's/^main_start pid=\([0-9]*\) .*/\1/p' "$2/sbx.status")
  [ -d "$cg" ] && [ -n "$inner" ] || return 1
  for p in $(cat "$cg/cgroup.procs"); do
    [ "$(awk '/^NSpid:/ {print $NF}' "/proc/$p/status" 2>/dev/null)" = "$inner" ] && { echo "$p"; return 0; }
  done
  return 1
}

cmd_term() {
  local p; p=$(cmd_pid "$1" "$2") || { echo "no SUT main process in case $1" >&2; return 1; }
  kill -TERM "$p"
}

cmd_stop() {
  local cg=$CG_ROOT/case-$1 p
  [ -d "$cg" ] || return 0
  for p in $(cat "$cg/cgroup.procs"); do kill -TERM "$p" 2>/dev/null; done
  for _ in $(seq 10); do grep -q 'populated 0' "$cg/cgroup.events" && return 0; sleep 0.1; done
  echo 1 >"$cg/cgroup.kill"
}

cmd_down() {
  local id=$1 out=$2 cg=$CG_ROOT/case-$1
  mkdir -p "$out"
  if [ -d "$cg" ]; then
    {
      echo "leftover_procs $(wc -l <"$cg/cgroup.procs")"
      echo "pids.peak $(cat "$cg/pids.peak" 2>/dev/null)"
      echo "memory.peak $(cat "$cg/memory.peak" 2>/dev/null)"
      grep -E '^(usage_usec|nr_throttled)' "$cg/cpu.stat"
      grep -E '^(max|oom_kill) ' "$cg/memory.events" "$cg/pids.events" 2>/dev/null | sed 's#.*/##'
    } >"$out/cgroup.txt"
    if ! grep -q 'populated 0' "$cg/cgroup.events"; then
      echo 1 >"$cg/cgroup.kill"
      for _ in $(seq 20); do grep -q 'populated 0' "$cg/cgroup.events" && break; sleep 0.1; done
    fi
    rmdir "$cg" || echo "cgroup_not_removed" >>"$out/cgroup.txt"
  fi
  ip netns del "sbx-$id" 2>/dev/null
  ip netns del "tn-$id" 2>/dev/null
  echo "down $id"
}

[ "$(id -u)" = 0 ] || { echo "sbx.sh must run as root" >&2; exit $BLOCKED; }
sub=${1:?subcommand}; shift
case $sub in
  up) cmd_up "$@" ;;
  run) cmd_run "$@" ;;
  stage2) cmd_stage2 "$@" ;;
  pid) cmd_pid "$@" ;;
  term) cmd_term "$@" ;;
  stop) cmd_stop "$@" ;;
  down) cmd_down "$@" ;;
  *) echo "unknown subcommand $sub" >&2; exit 2 ;;
esac
