#!/bin/bash
# Runs as the SUT inside the sandbox and reports what it can see and do (WP0.7 acceptance).
# One "key value" line per property on stdout.
echo "uid $(id -u)"
echo "groups $(id -G)"
echo "pid $$"
echo "hostname $(hostname)"
echo "cwd $(pwd -P) entries $(ls -A . | wc -l)"
grep -E '^(NoNewPrivs|CapEff|CapBnd):' /proc/self/status | tr -d '\t' | sed 's/:/ /'
echo "cgroup $(cat /proc/self/cgroup)"
echo "procs_visible $(ls /proc | grep -c '^[0-9]')"
for p in /home/obs/e0 /home/obs/.ssh /opt/e0/runs /root/.bashrc /mnt/fx/secret /etc/shadow; do
  if [ -r "$p" ] && ls "$p" >/dev/null 2>&1; then echo "readable $p yes"; else echo "readable $p no"; fi
done
sudo -n true 2>/dev/null && echo "sudo yes" || echo "sudo no"
echo "routes $(ip -o route show | tr '\n' ';')"
try() { timeout 3 bash -c "echo probe >/dev/tcp/$1/$2" 2>/dev/null && echo "connect $1:$2 yes" || echo "connect $1:$2 no"; }
try 10.77.0.1 8081
try 10.77.0.3 9090
try 192.168.68.76 22
try 192.168.68.1 53
try 1.1.1.1 443
# Directories on the root filesystem the SUT could write to (would persist across cases).
echo "rootfs_writable_dirs $(find / -xdev -writable -type d 2>/dev/null | tr '\n' ' ')"
echo "other_writable_dirs $(find /run /dev -xdev -writable -type d 2>/dev/null | tr '\n' ' ')"
echo "fixture_writable $( [ -w /srv/fx/out ] && echo yes || echo no )"
echo "sut_bin_writable $( [ -w /sut ] && echo yes || echo no )"
echo "nofile $(ulimit -n) fsize $(ulimit -f) core $(ulimit -c)"
