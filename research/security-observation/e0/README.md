# E0 観測器の校正(第1段)

SEC-OBS-01 の E0(DESIGN.md 11.1)のうち、**観測器そのものの校正**を専用 VM `sec-obs-01` で行った記録。2026-10-19。

- 対象: strace(B の collector)、Nulgrind・Memcheck(C0・C)、正解ラベル用の二層(fanotify = 有無、VFS 関数に掛けた bpftrace = byte 数)。
- SUT は校正用の C プログラム `cal.c` だけ。実験用の SUT(V/R/H)はまだ無く、公開ケースも保留ケースも実行していない。
- 判定は `check.py`。各 run の各層を期待値と突き合わせ、exit code で合否を返す。

## 再現

```sh
# VM 上(~/e0 に cal.c fanmon.c vfs.bt run.sh check.py を置く)
./run.sh runs/rN        # 49 run、約1分40秒
python3 check.py runs/rN            # 校正 run が全層で期待どおりなら exit 0
python3 check.py runs/rN --mutate   # 期待値を5か所壊し、全部検出できれば exit 0
```

| ファイル | 役割 |
|---|---|
| `cal.c` | 校正項目ごとのサブコマンド(fdreuse, fail, kxfer, child, symlink, restore, late, normal, noop, heapover, uaf, logical) |
| `fanmon.c` | fanotify の監視(root、tmpfs 全体に `FAN_MARK_FILESYSTEM`) |
| `vfs.bt` | VFS 関数の fexit で inode ごとの byte 数を出す。`vfs_copy_file_range` の内側の splice は数えない |
| `run.sh` | run ごとに新しい tmpfs の fixture を作り、正解ラベルの二層を起動してから SUT を1回動かす |
| `check.py` | 期待値との照合と、期待値を壊したときに検出できるかの自己試験 |
| `runs/r1` | 1回目。手順に2つ誤りがあった(下の「1回目で見つけた誤り」)。照合には使わない |
| `runs/r2`, `runs/r3` | 修正後の2回。結果は同じ |

版(`runs/r3/env.txt`): kernel 6.12.113+deb13-amd64、strace 6.13、valgrind 3.24.0、bpftrace 0.23.2、gcc 14.2.0。`cal.c` の sha256 は `3294914e…`。

## 結果(r2・r3 の2回。各 run は1回ずつ)

`check.py runs/r2` と `runs/r3` はどちらも exit 0 で、49 run のうち 47 が OK、2 が EXPLORE(期待値そのものを調べている run)だった。`--mutate` では期待値を壊した 11 run をすべて検出した(exit 0)。

| 校正項目(11.1) | 結果 |
|---|---|
| FD の再利用・dup | 閉じた FD 番号を別ファイルが再利用し、さらに dup しても、strace(`-yy`)・VFS・fanotify の三層とも secret 10 byte、decoy 25 byte と正しく分けた |
| 失敗する open/read/connect | strace は ENOENT・EACCES・EBADF・ECONNREFUSED を試行として記録し、成功の byte は0。VFS・fanotify には何も出ない(失敗した open は VFS に届かない)。connect の正解ラベルは今回の二層では取れない |
| カーネル内転送 | sendfile・copy_file_range・splice のどれも、三層で入力 secret 12,288 byte、出力3ファイル各 4,096 byte と一致した |
| 子プロセスと exec | `strace -f` は fork+execve と、glibc 2.41 の `posix_spawn` が使う `clone3` の両方を追った。子の `cat`(coreutils)は read ではなく **copy_file_range** でコピーしていた |
| fixed symlink | strace は `-yy` で解決先 `/mnt/fx/secret` を表示し、fanotify も解決先のパスで届いた |
| 一時書込みと復元 | 最後の hash は最初と同じなのに、三層とも書込み 8,192 byte を記録した |
| 応答後の副作用(代用) | 親が終わった 300 ms 後に、切り離した子が secret を読む。名前空間なしでは三層とも記録した |
| ヒープ越境・解放後使用 | `-O0` では Memcheck が InvalidWrite・InvalidRead を出した。**`-O2` ではヒープ越境が消えた**(下)。解放後使用は `-O2` でも出た |
| 論理領域だけの違反 | 同じ割当ての中で隣のフィールドを読む違反に、Memcheck は何も出さなかった(想定どおりの限界) |
| 正常な補助操作 | public の読みと out/ok の書込みだけが出て、過剰な記録は無い |
| Nulgrind | どの項目でもエラーを出さない。fixture への作用は直接実行と同じ |
| 正解ラベル用追跡器 | すべての校正 run で strace と二層の値が一致し、裁定が要る不一致は出なかった |

## 名前空間と Valgrind について分かったこと(DESIGN.md 8.2・9.6 の [U])

- **strace は user・pid・net 名前空間を越えて追えた**(`strace -f unshare -Urn --pid --fork …`)。ただし行頭の pid は strace 側の名前空間の値で、`clone` の戻り値は SUT 側の値(例: 2)だった。**clone の戻り値で親子を結ばない。**
- **pid 名前空間の中では、切り離した子が SUT 本体の終了と同時に SIGKILL された**(`ns-strace-late`)。三層とも secret の読みを記録しなかったので、観測の誤りではなく、隔離のせいで SUT の振る舞いが変わった例である。sandbox では SUT を名前空間の pid 1 にしないか、観測期間の終わり方を事前に決める必要がある。
- Valgrind(Memcheck)は user・pid 名前空間の中でも動き、ヒープ越境を検出した。
- `--xml=yes` を付けると、`--log-file` のテキストログは空だった。テキストログが要るなら XML と別の run にするか、XML だけを使う。
- strace を Valgrind の外側に付けると、Valgrind 自身の open(debuginfo、`.valgrindrc`)も SUT と同じ pid で出る。**pid ではなく対象リソースで絞る。**
- fanotify は strace なしの直接実行で、open・access・close を1イベントにまとめて届けることがあった(secret は mask 0x31 の1件、decoy は 0x21 と 0x10 の2件。r2・r3 とも同じ)。有無の判定には影響しないが、回数は数えられない。

## 1回目で見つけた誤り(r1。r2 で修正)

1. **`-O2` のビルドで、`heapover` の malloc と free が最適化で消えていた。** Memcheck は何も出さなかった。7.2 の「最適化で経路が消える」が校正用プログラムで起きた形で、witness の確認が要ることを示す。メモリ系の項目は `-O0` でもビルドして流すように変えた。`-O2` の run は EXPLORE として残してある。
2. **controller が fixture を読んで atime を動かしていた。** `rw` の hash を atime を戻した後に取っていたので、`rw` の atime が更新されていた。hash を取ってから atime を戻す順に直した(9.1 の「controller は最初の要求より前に fixture を読まない」に当たる)。

## やっていないこと

- **ログ欠落・parser 障害**: B の parser(WP3)がまだ無い。`check.py` の strace 解析は校正用の簡易版で、B の collector ではない。
- **実装の syscall 列(9.2)**: 実験用の実装(WP1)がまだ無い。
- **atime の strictatime と relatime の違い**: atime を 2020 年に戻してあったので、どちらのモードでも最初の読みで更新された。2回目の読みでの違いは見ていない。
- **fanotify のキューあふれ**(FAN_Q_OVERFLOW)は起こしていない。
- **通信系とプロセス系の正解ラベル**(audit の connect・execve 記録、受信器)は作っていない。
- **Valgrind の `--trace-children`** と、Valgrind 上の `child` 項目は流していない。
- **費用**(実行時間・ログ量)は測っていない。`time.txt` に各 run の開始と終了の時刻だけを残した。
- 各 run は1回ずつで、それを2回(r2・r3)繰り返しただけ。一般化しない。
