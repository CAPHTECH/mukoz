# WP3 第1段 strace → 正規化イベント(と WP4 の Memcheck)

条件 B の観測(strace)の記録を正規化イベント(`secobs.observations/0`)に変える。出力は `adapter/adapter.py` で `mukoz.external_claims/0` になる(DESIGN.md 9.2〜9.7、16.1、17章 WP3)。2026-10-19 に、E0 と WP0.7 で取った実際の trace で校正した。**シナリオ F01〜F12 の fixture と実装(WP1)はまだ無いので、F03 などの実際の違反と、実装の syscall 列は試していない。**

```
strace.py --binding B --case ID=TRACE [--case ...] --principal P --requests-sent N
          --fx PREFIX [--fx ...] [--sut-exec PREFIX] [--controller-end END] -o OUT
```

取り方: `strace -f -yy -q --decode-pids=pidns -s 0 -e trace=%file,%desc,%network,%process -o TRACE <cmd>`。WP0.7 の受入試験で使った `-qq` とコメントなしの記録も読める(下の「pid の対応付け」)。

## 何をどう読むか

| 9.2 の領域 | 読む syscall | イベント |
|---|---|---|
| ファイル取得 | open・openat・openat2・creat | `file_open`(access は read / write / rw。`O_PATH` は none) |
| 内容の I/O | read・pread64・readv・preadv(2)、write・pwrite64・writev・pwritev(2) | `file_read` / `file_write`(正の戻り値が byte 数。失敗は errno つき) |
| カーネル内転送 | sendfile・copy_file_range・splice | `file_transfer`(転送元と転送先) |
| FD 系譜 | close・dup 系・fcntl(F_DUPFD 系、F_SETFD)・clone 系・exec(O_CLOEXEC) | イベントは出さず、resource の照合に使う |
| 通信 | connect(IPv4・IPv6・unix) | `net_connect`。拒否も EINPROGRESS も「試行」で、成立にはしない。EINPROGRESS は結果を追わないので、禁止先なら「成立していない」とも言わない(adapter が INCONCLUSIVE `OUTCOME_UNOBSERVED`) |
| プロセス | execve・execveat、clone・clone3・fork・vfork、終了記録 | `process_exec`、SUT の範囲、観測の終わり |
| 書換え・未対応の経路 | rename・unlink・truncate・chmod 等、ファイルの mmap、io_uring、vmsplice、SCM_RIGHTS、process_vm_* | `unsupported_mechanism`(対象の resource と op)。adapter は保護対象に関わる property を INCONCLUSIVE にする |

- **resource の同定**: `-yy` の fd 表示を、parser が持つ FD 系譜と突き合わせる。どちらか一方しかなければその値を使う。食い違ったとき、または `(deleted)` と表示されたときは未解決(`resource_id` null)にする。失敗した open は fd が無いので、渡された path(相対 path は dirfd の表示を前に付ける。`..` を含むものは未解決)。`--fx` の接頭辞の下は fixture 内の相対名に、それ以外は path のままにする。inode では照合していない(strace の表示に inode は無い)。9.3 の固定 namespace の前提に頼っている。
- **SUT の範囲**: `--sut-exec` の接頭辞に一致する最初の成功した execve から始まり、その process と、それ以後に作られる子孫が SUT になる。それより前の作用(sandbox の supervisor、setpriv など)は `supervisor` として数えない。
- **pid の対応付け**: pid 名前空間の中の clone は、内側の pid を返す。そのため trace の pid と一致しない(E0、8.2)。`--decode-pids=pidns` の注記があればそれを使う。無ければ、割り当て順(両方の名前空間で pid が増える)で、対応の付いていない呼び出しと子を並べて対にする。[R] pid の一周や、入れ子の名前空間が複数あると崩れる。対応の付かない子は役割不明とする。そのイベントは反例にならないが、SATISFIED も妨げる(`pid_unmatched`)。
- **receipt**(9.5・9.7): 開始は SUT の exec が見えたか。終了は、SUT のすべての process に終了記録があるか、SIGKILL による強制停止(`forced_stop`)が無いか、controller の申告(`--controller-end`)が drained か。欠落として数えるもの: 読めない行、始まりの無い resumed 行、再開されない unfinished、`strace:` の行、I/O の戻り値が `?`、`restart_syscall`、最後の改行の欠落。SIGSEGV などによる SUT 自身の終了は、観測の終わりとして扱う。

## 試験

```sh
python3 test_strace.py                          # 46 本。記録した trace を壊したものと、小さな合成 trace
python3 e0_check.py ../e0/runs/r2 ../e0/runs/r3 ../sbx/runs/s5 ../sbx/runs/s6 ../sbx/runs/p1
python3 mutate.py                               # strace.py を 20 通り壊し、上の2つのどちらかが落ちることを確かめる
```

`e0_check.py` は、strace の記録がある run ごとに2つの表を照合する。(1) SUT の正規化イベントから集計した、fixture のファイルごとの byte 数・open したファイル・失敗した試行が、`e0/check.py` の EXPECT と等しいこと。EXPECT は E0 と WP0.7 で、正解ラベルの二層(fanotify・VFS の bpftrace)と一致した期待値。(2) `e0_binding.json`(principal `cal` は secret に触れてはならず、rw は読取りだけ)で adapter に通した判定が、項目ごとの期待値に等しいこと。grace 0 の run は、どの property も SATISFIED にならないことだけを見る。

2026-10-19 の結果(各1回):

- `e0_check.py`: 60 run すべてが一致した(E0 の r2・r3 で 30、WP0.7 の s5・s6 で 20、新しく取った p1 で 10)。
- `test_strace.py`: 46 本すべて通った。
- `mutate.py`: exit 0。20 通りの壊し方はすべて検出された。
- p1 は専用 VM で `SBX_SECTIONS="3 4" SBX_STRACE_OPTS="-f -yy -q --decode-pids=pidns …" ./test.sh runs/p1` として取った(sandbox を通した校正 9 項目と grace 0)。`e0/check.py` での三層の照合も 9 項目とも一致した。同じ 10 本から注記を取り除き、割り当て順の対応付けを使った結果は、注記による対応 394 件のすべてで一致した。

### 作る途中で見つけて直したもの

1. 行頭の pid の後の空白が2つある行(E0 の r2)を読めず、SUT の exec を取り逃していた。
2. 戻り値の後の `(Timeout)` や `(flags …)` を読めなかった。
3. pid 名前空間の中の子を親に対応付けられず、sandbox の SUT の作用が「役割不明」になっていた(割り当て順の対応付けで直した)。
4. Valgrind の `./.valgrindrc` の失敗した open を、fixture の中のファイルとして数えていた(相対 path を dirfd で解決するようにした)。
5. rename で移動元しか見ておらず、上書きされる移動先を見落としていた(合成 trace の試験で見つけた)。
6. 故障注入の初回では 20 通りのうち 6 通りを見逃した。試験が別の理由で INCONCLUSIVE になる形だったもの(4件)と、注入が挙動を変えていなかったもの(2件)。試験と注入を直した。

## やっていないこと

- **シナリオ F01〜F12 と2系統の実装**(WP1)。F03(読んでから 403)などは、この parser にはまだ通していない。実装の syscall 列(9.2 の必須集合の外の経路を使うか)も未確認。
- **request への帰属と段階**(9.4): 起動・ready・request・終了を分けていない。観測期間は SUT の exec から終了記録まで。
- **thread**: `CLONE_THREAD` と fd 表の共有は実装したが、記録した trace には thread が無く、合成 trace でも試していない。
- **inode による同定**(9.3)、同時の path 置換、`(deleted)` 以外の再利用。
- **出力量の上限と strace 自身の取りこぼし**: strace が自分で捨てた記録を知る手段は、上の欠落の検出だけ。9.7 の `trace_storage_truncated` に当たる `-s` の省略は、byte 数と fd 表示には影響しない(path 文字列が切れたら未解決にする)。
- 別系列のモデルによるレビュー。

## Memcheck(WP4、`memcheck.py`)

Memcheck の XML(`--xml=yes --xml-fd=4`)を、`mem_error` イベントと receipt に変える。取り方は `valgrind --tool=memcheck --xml=yes --xml-fd=4 --error-limit=no --trace-children=no --leak-check=no`(`fx/run.sh` の `FX_MODE=c`)。

- **イベント**: `<error>` ごとに1つ。`detail` は Memcheck の kind、what、auxwhat(allocation との関係)、先頭の frame(ip・obj・fn・file・line)、allocation の stack の先頭 frame、`<errorcounts>` の回数。Memcheck は同じ場所の誤りを1回だけ報告する。module offset は XML に load base が無いので出していない [R]。
- **receipt**: 開始は preamble と client の argv があること。終了は、最後の `<status>` が FINISHED で、`</valgrindoutput>` があること。欠落として数えるもの:
  - XML の途中切れ(切れる前の完全な `<error>` は反例として残す)
  - `--error-limit=no` を付けていない(報告が途中で止まり得る)
  - suppression が一致した(SUT のコードかどうか分からない)
  - client の SIGILL(未対応の命令と SUT 自身の SIGILL を区別できない [R])
  - tool が memcheck でない
  - SIGTERM による停止は、欠落にしない。
- **B との合流**: adapter は case ごとに receipt を1つしか持たないので、strace と Memcheck の receipt を保守的に合わせる(`merge`)。どちらかの欠落や終了の欠如は、その case のすべての claim を保留させる。
- 試験: `python3 test_memcheck.py`(14 本。記録済みの XML とそれを壊したもの)、`python3 test_memcheck.py --mutate`(memcheck.py を 9 通り壊す)。2026-10-19 に、14 本すべて通り、9 通りすべてを検出した(各1回)。
