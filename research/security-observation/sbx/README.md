# WP0.7 SUT sandbox

SEC-OBS-01 の SUT を動かす sandbox(DESIGN.md 8.2・8.4、17章 WP0.7)。専用 VM `sec-obs-01` で 2026-10-19 に作って受入試験を行った。現行 `native-process` の隔離は使っていない(8.2)。

## 作り

controller が root で準備し、SUT だけを非特権に落とす。

```
sbx.sh up <id>      cgroup /sys/fs/cgroup/sec-obs/case-<id>(pids 256、memory 1G、cpu 2コア分)
                    netns sbx-<id>(SUT、10.77.0.2/24、既定経路なし)と tn-<id>(試験用ネットワーク)
                    tn-<id> に 10.77.0.1(許可された受信器)と 10.77.0.3(禁止扱いの受信器)
sbx.sh run <id> <out> <fixture> <bin> <grace-ms> -- <cmd>
   supervisor が case cgroup に入る
   → ip netns exec sbx-<id> unshare --mount --pid --ipc --uts --fork
   → stage2(新しい名前空間の pid 1、root):
        /proc を張り直す、<fixture> を /srv/fx に、<bin> を /sut に読み取り専用で bind
        /home /root /mnt /media /tmp /var/tmp /dev/shm /run を case ごとの tmpfs で隠す
        mqueue を張り直す、hostname を変える、cwd を / にする
        検査(下)。1つでも外れたら exit 3 で、SUT は起動しない
   → setpriv(uid/gid obs、補助グループなし、no_new_privs、capability 全部なし)
   → prlimit(NOFILE 1024、FSIZE 64 MiB、core 0)
   → sbxinit <grace-ms> <cmd>(pid 1 のまま。SUT は pid 2 以降)
sbx.sh pid <id> <out>    SUT の main process の外側の pid(sbxinit が status に書く main_start で探す)
sbx.sh term <id> <out>   SUT の main process だけに SIGTERM(常駐する SUT の停止。WP1 で追加)
sbx.sh stop <id>    cgroup 内の全プロセスに SIGTERM、1秒後に cgroup.kill
sbx.sh down <id>    残ったプロセス数・pids/memory/cpu の peak を記録し、cgroup と netns を消す
```

- **fixture は、外側から見えるのと同じ superblock** を bind している。正解ラベルの二層(fanotify・VFS の bpftrace)は、外側からそのまま同じ inode を見られる。
- **診断の出力先**: `SBX_DIAG=<file>` を付けると、controller 側のファイルを fd 4 として渡す(例: `valgrind --xml-fd=4`)。SUT の stderr と混ざらない。
- **status**: fd 3(`<out>/sbx.status`)に、検査の通過、SUT の終了、猶予の結果を書く。SUT には fd 3 を渡さない。

### pid 1 の扱い(8.2 の決定)

**SUT を pid 1 にせず、待つだけの init(`sbxinit`)を pid 1 に置く。** SUT 本体が終わってから `grace-ms` の間は子孫を待ち、それを過ぎたら sbxinit が終わる。その時点で名前空間に残っている子孫は、カーネルが終了させる。

`grace-ms` は事前登録する実験パラメータとする。観測期間は「SUT 本体の終了 + grace-ms」で終わる。E0 では、切り離した子が 300 ms 後に読む項目を、grace 1000 ms では三層とも記録し、grace 0 では読む前に終了させた(2回)。

## 起動前の検査(1つでも外れたら BLOCKED)

| 検査 | 方法 |
|---|---|
| controller 領域が見えない | `/home/obs/e0`・`/home/obs/.ssh`・`/opt/e0/runs` が存在しない |
| 外への経路が無い | `ip route` に既定経路が無く、10.77.0.0/24 以外の経路も無い |
| /proc が case の pid 名前空間のもの | `/proc/1/cmdline` が自分(stage2)である |
| case cgroup に入っている | `/proc/self/cgroup` が `0::/sec-obs/case-<id>` |
| cwd が隠した領域を指していない | `cd /` 後に `pwd -P` が `/` |

## 受入試験

```sh
# VM 上、~/sbx と ~/e0 が並んでいる状態で(obs、sudo 可)
./test.sh runs/sN            # 15 run、約1分20秒
python3 check_sbx.py runs/sN # 41 項目。全部満たせば exit 0
```

環境変数 `SBX_SECTIONS`(例 `"3 4"`)で節を選び、`SBX_STRACE_OPTS` で strace の options を変えられる(WP3 の `runs/p1` は `-f -yy -q --decode-pids=pidns -s 0 -e trace=%file,%desc,%network,%process` で取った)。

`check_sbx.py` は、SUT として動かす `probe.sh` の報告、`e0/check.py` の三層照合、cgroup の記録を見る。

**結果**: s5・s6 の2回とも exit 0 で、41 項目すべてを満たした。主な項目は次のとおり。

- SUT は uid 1000、補助グループなし、no_new_privs、CapEff と CapBnd は 0、pid 1 ではない。
- 次のものが読めない: `/home/obs/e0`、`.ssh`、`/opt/e0/runs`、`/root/.bashrc`、外側の `/mnt/fx/secret`、`/etc/shadow`。`sudo -n` は失敗した。
- 許可された受信器と禁止扱いの受信器には、どちらも OS 上は接続でき(8.3)、受信器の記録にも残った。作業用ホスト・ゲートウェイ・1.1.1.1 には届かなかった。
- 書けるディレクトリは、case ごとの tmpfs と fixture だけだった。`/sut` は読み取り専用。
- 検査を壊すと(`SBX_INJECT=blocked`)exit 3 になり、SUT は起動しなかった(stdout は空で、`main_exit` も無い)。
- E0 の校正項目9つは、sandbox を通しても strace と二層が期待どおりに一致した。strace は sandbox の外の root から `-f` で追い、SUT の `execve` より前の supervisor の作用は scope の外として数えない。
- Memcheck は sandbox の中でヒープ越境を検出し、XML は fd 4 経由で controller 側へ出た。
- 400 プロセスを起動しようとすると pids.max で止まり(`pids.events:max 2`、peak 256)、終了後に残ったプロセスは0だった。

## 受入試験で見つけて直したもの(s1〜s4。記録は VM の `~/sbx/runs` に残してある)

1. **SUT が `/dev/mqueue`、`/run/screen`、`/run/cloud-init/tmp` に書けた**(s1・s2)。どれもホストと共有で、次の case まで残る。IPC と UTS の名前空間を分け、mqueue を張り直し、`/run` 全体を case ごとの tmpfs にした。
2. **SUT の cwd が controller の `/home/obs/sbx` のままだった**(s4 の strace で `AT_FDCWD</home/obs/sbx>` を見て気づいた)。`/home` を tmpfs で隠しても、開いたままの cwd からは相対パスで記録を読めた。stage2 で `cd /` し、検査にも加えた。
3. **判定器の誤り**(2件、sandbox の欠陥ではない): supervisor の作用(setpriv の NSS 問い合わせの connect)を SUT の作用として数えていた。SUT の `execve` が strace で unfinished と resumed の2行に分かれたとき、scope の開始を取り逃していた。どちらも `e0/check.py` を直し、E0 の r3 が引き続き exit 0 になることを確かめた。

## やっていないこと

- **HTTP サービスの「起動 → 要求 → 停止」**: WP1(`fx/`)で、常駐する SUT を「LISTEN を待つ → request を送る → main process に SIGTERM」で止める形を 144 run 動かした。そのために sbxinit が status に `main_start` を書くようにし、`pid`・`term` を足した。変更後に受入試験を再実行し(`runs/s7`)、41 項目すべてを満たした。wall time と出力量の上限は、まだ無い。
- **mount option の検査**: strictatime かどうかは検査していない。`findmnt` の表示に strictatime が出ないので、relatime・noatime が無いことで判定する必要がある。
- **disk・出力量の予算**: FSIZE と tmpfs の size だけで、stdout の量の上限は無い。wall time の上限は controller 側(`timeout`)に任せていて、まだ付けていない。
- **seccomp と Landlock**: 主比較では使わない(8.3)。14章の強制制御つき条件で別に扱う。
- **カーネル・名前空間の突破への耐性**: 8.2 のとおり、主張しない。
- 受入試験は2回(s5・s6)。一般化しない。
