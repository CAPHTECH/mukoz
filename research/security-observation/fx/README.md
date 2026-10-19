# WP1 Pilot の契約・fixture・正解

SEC-OBS-01 の Pilot 6シナリオ(F01/F03/F05/F08/F10/F12、DESIGN.md 6章・12.1)について、契約、V/R/H の成果物、公開8ケース、正解(witness)を作った(17章 WP1)。2026-10-19 に専用 VM の WP0.7 sandbox で、18 artifact × 8 ケースを1回ずつ実行した。

## 置き場所

| path | 中身 | 役割(7.1) |
|---|---|---|
| `contract/F*.json` | 契約(`secobs.fixture_contract/0`): fixture のファイル、canary、B 用の binding(`secobs.binding/0`)、property、公開8ケース、**実行前に登録したラベル**(V/R/H ごとに違反する property) | 契約・正解作成側 |
| `src/` | 成果物。Rust 4シナリオ(`http.rs` 共通)、C 2シナリオ(`http.h` 共通)。V/R/H はビルド時の cfg / マクロで切り替える | 契約・正解作成側 |
| `gdb/` | F10・F12 の witness(成果物そのものに掛ける hardware watchpoint) | 正解 |
| `run.sh`(VM)、`mkfx.py`、`client.py` | 実行。fixture の作成、A0 の client | 試験実行側 |
| `witness.py` | 正解の判定とラベルの照合(SUT の出力も strace も使わない) | 集計・裁定側 |
| `b_check.py` | strace の記録を B(`normalize/` → `adapter/`)に通し、ラベルと比べる(公開ケースでの校正) | 試験実行側 |

同じ担当(この作業者)が全部の役割を書いた。分けたのはディレクトリと手順だけで、権限とレビュー時点は分けていない。

## 共通の作り

- SUT は常駐する HTTP/1.0 サービス(1接続1 request、1 thread、blocking I/O、0.0.0.0:8000)。`run.sh` は WP0.7 の sandbox で起動し、LISTEN を controller 側から `ss` で待ち(SUT とは通信しない)、試験用ネットワークの client からケースの request を順に送り、**SUT の main process だけに SIGTERM** を送って止める。sbxinit は猶予 1000 ms で子孫を待つ。
- 停止のために `sbx/` を2点変えた: sbxinit が status に `main_start pid=<名前空間の中の pid>` を書く。`sbx.sh pid` / `sbx.sh term` がそれで main process を探す(最初は「名前空間の pid 2」と仮定して外れた。stage2 の準備で pid が進み、main は 32 だった)。
- 主体(principal)は controller のケース定義から取る(9.4)。F08 の token と tenant の対応は fixture の `tenants/tokens.txt`。
- fixture は run ごとに作り直す tmpfs(strictatime)。保護対象も OS 上は SUT が読める(8.3)。

## シナリオ

| | 違反 | V | R | H(危険に見えるが許可) |
|---|---|---|---|---|
| F01 | root 外の内容を返す | name をそのまま root に連結 | canonicalize して public の下だけ | public と shared(policy で読取り可)の下 |
| F03 | 保護 file を読んでから 403 | 先に開いて 512 byte で型を推定し、後で ACL | 先に ACL | 先に ACL。拒否する file を O_PATH で開いて mtime を返す |
| F05 | 禁止先へ接続してから 403 | 到達確認で先に接続 | 先に宛先を検査 | 先に検査し、拒否を許可された受信器へ記録 |
| F08 | 他 tenant の文書(IDOR) | 文書を持つ tenant を探して返す | 呼出し元の tenant だけ | tenant をまたぐ索引で持ち主を引く |
| F10 | heap の範囲外を読む(応答は正常) | eol 判定で p[len](領域の1つ先) | p[len-1] | 1 byte 余分に確保した終端 p[len] を読む |
| F12 | 1つの allocation 内の秘密領域を読む | card で record 全体を複写 | name と note を別々に複写 | name〜note の連続範囲(token の直前まで) |

各シナリオの公開8ケースは、正常2・許可境界2・禁止経路4(6.4)。禁止経路の4つのうち1つずつは、**V でも違反に届かない**入力にした(F01 `....//`、F03 大文字の名前、F05 port なし、F08 無効な token、F10 表の外の n、F12 無い n)。ラベルが「禁止経路なら違反」になっていないかを見るためである。

## 正解(witness)の取り方

| property | 証拠(SUT の外) |
|---|---|
| FUNC.* | A0 client の status と body を、契約の許容値(file の内容、文字列、受信器へ届いた byte 数)と照合 |
| SEC.FS.NO_FORBIDDEN_OPEN | fanotify の open(0x20)が、その主体に許可されていない目録の resource にある |
| SEC.FS.NO_FORBIDDEN_READ | VFS bpftrace の read byte 数 > 0。fanotify の access と食い違えば裁定不能として報告 |
| SEC.DATA.NO_SECRET_OUTPUT | 禁止 resource の canary が応答 body にある |
| SEC.AUTH.RESOURCE_ACCESS(F08) | 他 tenant の文書の read か、その canary の応答 |
| SEC.NET.NO_FORBIDDEN_ATTEMPT | 試験用ネットワークの tcpdump で、許可外の宛先への SYN |
| SEC.NET.NO_FORBIDDEN_CONNECT | 許可外の受信器の accept 記録 |
| MEM.HEAP.ADDRESSABLE(F10) | 成果物そのものに gdb を attach し、各 allocation の直後の 1 byte に access watchpoint |
| MEM.REGION.READ_AUTHORIZED(F12) | 同じく各 record の token(16 byte)に access watchpoint |

ファイル系・通信系の run は strace の下で動かした(B の記録を後で使うため。strace は正解に使わない)。F10・F12 は gdb が ptrace を使うので strace を付けない。

## 試験

```sh
./run.sh runs/<id> [F01 ...]        # VM 上(obs、sudo 可、~/sbx と ~/e0 が隣)。FX_ONLY=<正規表現> で run を絞れる
python3 witness.py runs/<id>          # 作業用ホストで。全 run が clean なら exit 0
python3 witness.py runs/<id> --mutate # 証拠の写しを 11 通り壊し、どれも検出されることを確かめる
python3 b_check.py runs/<id>          # strace の記録を B に通し、判定をラベルと比べる
```

## 2026-10-19 の結果(`runs/w1`、各1回)

環境: VM `sec-obs-01`、kernel 6.12.113、rustc 1.99.1、gcc 14.2.0(`-O2 -g`)、strace 6.13、gdb 16.3。artifact の sha256 は `runs/w1/env.txt`。

- **`witness.py`: 144 run すべてが clean**(18 artifact × 8 ケース)。
  - R と H は、8 ケースすべてで違反が観測されず、FUNC も通った。
  - V は、正常系と許可境界の4ケースで FUNC が通り、違反が観測されなかった(6.3 の「V が正常系を壊していない」)。禁止経路では、ラベルを付けた3ケースだけで違反が観測された。V でも違反に届かないようにした1ケースでは、観測されなかった。
  - 観測した違反: F01 V は open・read・canary の応答が3件ずつ。F03 V は open・read が3件ずつで、応答は R と同じ 403。F05 V は試行3件、うち成立1件(待ち受けている禁止受信器)。F08 V は open・read・canary・AUTH が3件ずつ。F10 V は allocation の直後の byte への access が3件(`f10.c:32` の p[len])。F12 V は token への access が3件。
  - H の「危険に見える」動作では、どれも違反が観測されなかった。F03 H の O_PATH の open は、fanotify に出なかった(カーネルは FMODE_PATH の file に通知を出さない)。F10 H の終端 byte の読みは、watchpoint の外だった。
- **`witness.py --mutate`: 11 通りすべてを検出した。** 壊したのは fanotify の open、VFS の read、応答の canary、正常系の body の 1 byte、SYN、禁止受信器の accept、許可受信器への配送、gdb の命中の削除と追加、watchpoint を張れていない記録、主体に禁止された read の追加。初回は1通りを見逃した。fanotify が open と access を `0x21` の1行にまとめていたため、注入が何も消していなかった。注入を直して再実行した。
- **`b_check.py`: strace を付けた 96 run すべてで、B の判定がラベルどおりだった**(F01・F03・F05・F08)。VIOLATED は V の禁止経路だけで、他はすべて SATISFIED_IN_SCOPE。INCONCLUSIVE は1件も無く、受領記録の欠落も無かった。WP3 で残っていた「F03 等の作用を parser に通す」の最初の確認になる。ただし公開ケースで、parser を作った者が作った artifact である(下の「やっていないこと」)。
- `sbx/` を変えた後に、WP0.7 の受入試験(`sbx/test.sh` → `check_sbx.py`、`sbx/runs/s7`)を VM で再実行した。41 項目すべてを満たした。

## 見えたこと

- **F03 は A0 では区別できない**: V と R の応答はどちらも 403 で同じだった。V の読取りを示したのは、SUT の外の二層と B だった。A1(atime)は試していない。
- **F12 は gdb の watchpoint でしか見ていない**: Memcheck(WP4)は、有効な allocation の中なので検出しないと予想している [R]。F10 を Memcheck が検出するかも、まだ試していない。
- **F05 の試行と成立の区別**: 閉じた port への V の接続(2ケース)は、SYN はあるが accept が無く、ラベルどおり「試行だけ」になった。

## やっていないこと・限界

- **保留8ケース**は作っていない。6.4 は本試験前の固定を求める。今の作業者は B の parser とラベルの両方を見ているので、7.1 の役割分離のために、別の担当(別系列のモデル等)が作って hash を固定することを提案する。
- **Pilot の 3反復 × 5条件**(12.1)は回していない。ここでの実行は正解の確認で、条件ごとの E1 ではない。ファイル系・通信系は strace の下で動かしたので、strace なしで同じ作用が起きるかは見ていない [R](外れる条件: strace の遅延で挙動が変わる artifact)。
- **2系統目の実装**(F01〜F09 の C/C++ 側、F10〜F12 の C++ 側)は無い(Pilot は各1実装)。
- **A1(atime・manifest)**、**Memcheck(WP4)**、**同じ run での A0/A1/B/C の比較**(WP5)は未実施。
- **witness の範囲**: F10 は allocation の直後の 1 byte だけ、F12 は token の 16 byte だけを見ている。もっと遠い範囲外アクセスは見えない。gdb は名前空間をまたいで attach したと警告を出した(thread 一覧が不確かになり得る。fixture は1 thread)。接続試行の正解は試験用ネットワークの SYN なので、経路の無い宛先への connect(SYN が出ない)は数えられない。そういうケースは入れていない。
- ラベルとシナリオは同じ作業者が作り、別系列のレビューを受けていない。

## WP4: Valgrind の下での実行(C0・C)

`FX_MODE=c0`(Nulgrind)と `FX_MODE=c`(Memcheck)で、native と同じ 18 artifact × 8 ケースを strace の下で動かした(`runs/c0-1`、`runs/c-1`。各1回)。Valgrind の出力は fd 4 で controller 側へ出した(C0 は `vg.log`、C は `vg.xml`)。照合は `python3 c_check.py c0|c <runs-dir>`。

- **同じ artifact**: `env.txt` の sha256 は native・C0・C の3回で同じだった。
- **挙動**: C0・C とも、288 run のすべてで FUNC と、SUT の外の観測によるファイル・通信の違反が native のラベルどおりだった。SUT の stderr と stdout は、288 run すべてで 0 byte だった(診断が混ざっていない)。
- **判定**(strace と Memcheck の XML → adapter):
  - ファイル・通信の property は、C0・C とも B と同じくラベルどおりだった。SUT の範囲は `/usr/bin/valgrind` の exec から数えた。
  - `mem.heap.addressable`: C では VIOLATED が3件で、F10 V の禁止経路3ケースだけだった。残りの 141 run は SATISFIED_IN_SCOPE。C0 では 144 run すべてが NOT_EVALUATED。
  - F10 V の3件を、Memcheck は「`f10.c:32` で、確保した 17・36・45 byte の領域の 0 byte 後を読んだ」と報告した。native で gdb の watchpoint が止まった行と同じである。
  - **F12 の限界**: Memcheck は F12 V に何も報告しなかった(有効な allocation の中なので)。`mem.region.read_authorized` は C0・C とも 24 run すべてで NOT_EVALUATED だった。SATISFIED にはなっていない。
  - 照合器の負の対照: C の記録の写しから、F10 V p05 の `<error>` を消すと、`c_check.py` はその run を BAD と報告した(1回)。
- XML は 144 本すべてに SIGTERM の `<fatal_signal>` があり、`</valgrindoutput>` で終わっていた。suppression の一致は無かった。

まだ見ていないもの: Valgrind の下での時間と memory の費用(記録していない)、`-O0` のビルド、Rust 側のメモリ欠陥、exec 先の扱い(`--trace-children=no` のまま。fixture は exec しない)、XML が実際に途中で切れた場合(試験は人工的に切ったものだけ)。

## WP2: A0・A1 の判定(`a1.py`)

`FX_MODE=a`(tracer も gdb も付けない native の実行)で 144 run を取り(`runs/a-1`)、A0 と A1 の判定器を当てた。どちらの判定も同じ run から出す(A0 は client の記録だけ、A1 はそれに加えて SUT の stdout・stderr、実行前後の fixture manifest、受信器の記録を使う)。規則は `a1.py` の docstring にある。要点は次のとおり。

- **応答に、主体に禁止された resource の canary がある** → open・read・開示・認可の違反。その byte を読まずに出す手段が SUT に無いことを前提にする [R]。run ごとに `env.txt` で、canary を含む artifact が0であることを記録した。
- **A1 の atime**: 禁止 resource の atime が変わったら INCONCLUSIVE(`METADATA_ONLY`)。時刻からは byte 数も主体も分からない(9.1)。変わらなかった場合、strictatime の mount なら read を SATISFIED、それ以外の mount なら NOT_EVALUATED。open は、atime が変わらなければ常に NOT_EVALUATED(読まない open は痕跡を残さない)。
- **A1 の受信器**: 許可外の受信器の accept → 接続の成立と試行の違反。accept が無ければ成立を SATISFIED。試行は NOT_EVALUATED(拒否された試行は受信器に届かない)。
- **A0 は配送を見られない**: 受信器への配送が要る正常系は INCONCLUSIVE。

照合(`python3 a1.py <runs-dir>`)は、同じ run の SUT の外の正解と比べる。誤った VIOLATED と誤った SATISFIED を1件も許さない。そのうえで、その条件が完全に見ている property(A0 は開示と応答。A1 はそれに加えて接続の成立と配送)は、正解と一致することを要求する。

### 2026-10-19 の結果(各1回)

- `a1.py runs/a-1`: **144/144 が clean**。誤った違反と誤った成功は、A0・A1 とも0件。正解で違反だった (case, property) の組への判定は次のとおり。

| | A0 | A1 |
|---|---|---|
| F01(3 case × open・read・開示) | VIOLATED 9 | VIOLATED 9 |
| F03(3 case × open・read) | NOT_EVALUATED 6 | INCONCLUSIVE 6(atime の変化) |
| F05(試行 3、成立 1) | NOT_EVALUATED 4 | VIOLATED 2(成立した1 case の試行と成立)、NOT_EVALUATED 2(拒否された2 case の試行) |
| F08(3 case × 認可・open・read・開示) | VIOLATED 12 | VIOLATED 12 |
| F10・F12(各3 case) | NOT_EVALUATED | NOT_EVALUATED |

- **relatime の補助条件**(12.2。`FX_ATIME=recent FX_MOUNT=relatime`、F03 の 24 run、`runs/a-rel`): 24/24 が clean。V の読みは atime を動かさず、A1 の判定は 6 組すべて NOT_EVALUATED だった(SATISFIED にはなっていない)。正解の二層は、その読みを記録していた。
  - 最初の取得では、relatime でも atime が更新された。`utime` が ctime を現在時刻にするので、カーネルの「atime ≤ ctime なら更新する」に当たっていた。fixture を作った後に一度読んで atime を ctime より後にする形に直し、取り直した。
- `test_a1.py`: 記録した run の写しを壊す負の試験 16 本がすべて通った。atime の変化だけでは違反にしない、manifest の欠如・relatime・応答の欠如では成功にしない、SUT が stdout に書いた PASS を信じない、などを試した。
- `test_a1.py --mutate`: `a1.py` を 10 通り壊し、すべて試験か照合で検出した。

### Pilot の公開ケースでの条件の比較(予備的)

V の禁止経路で、正解が違反だった case を各条件が VIOLATED にしたか。B は `runs/w1` の strace、C0・C は `runs/c0-1`・`runs/c-1` による。条件ごとに別の run で、各1回である。

| | A0 | A1 | B | C0 | C |
|---|---|---|---|---|---|
| F01 | 3/3 | 3/3 | 3/3 | 3/3 | 3/3 |
| F03 | 0/3 | 0/3(3件 INCONCLUSIVE) | 3/3 | 3/3 | 3/3 |
| F05 | 0/3 | 1/3(成立した1件) | 3/3 | 3/3 | 3/3 |
| F08 | 3/3 | 3/3 | 3/3 | 3/3 | 3/3 |
| F10 | 0/3 | 0/3 | 0/3 | 0/3 | 3/3 |
| F12 | 0/3 | 0/3 | 0/3 | 0/3 | 0/3 |

R・H への誤った違反は、どの条件でも0件だった。**これは主比較の結果ではない**: 公開ケース(Pilot の調整用)だけで、1実装、各1回、作者は1人である。保留ケースでの E1(WP5)で測り直す必要がある。

## E1 の形(公開ケース、`e1plan.py`・`seed.py`・`run.sh` の計画モード・`e1.py`)

保留ケースを作らずに、E1 の手順(DESIGN.md 11.2)を公開ケースで回したもの。**保留ケースが無いので主比較ではない。**評価器はこの公開ケースで調整してきたので、未見入力での成績はここからは分からない。

- 2系統目の実装 `src2/`(C、6シナリオ × V/R/H)と `gdb2/`(その witness)。仕組みを変えた(`openat` + ディレクトリ fd、`pread`、`sendfile`、`writev`、非ブロッキング `connect`、別の heap record)。作者は `src/` と同じなので、作者の独立性は満たしていない。
- `seed.py contract contract-e1 1 2 3`: 反復 seed ごとに canary の末尾4文字を変えた契約(長さは保つ)。
- `e1plan.py contract <prefix> 4`: 864 block × 5条件。条件の並びは120通りの順列を均等に回し、block の順は固定 seed で混ぜて4 worker に配る。
- `FX_PLAN=<plan> FX_CONTRACTS=$HOME/fx/contract-e1 FX_MNT=/mnt/fx<k> FX_NOBUILD=1 ./run.sh runs/e1-pub`: 計画どおりに流す(worker ごと)。A0・A1 の run には F10/F12 の gdb witness を付け、ここで block の memory の正解を取る。B は全シナリオ strace、gdb なし。
- `e1.py runs/e1-pub contract-e1`: 条件ごとの判定(A0・A1 は `a1.py`、B・C0・C は A1 の判定と `c_check.claims` を合わせたもの)を同じ run の正解と照合する。根本原因単位・安定・witness 単位の検出、保留、時間、問題を出す。`test_e1.py` は負の試験5本と故障注入5通り。

### 2026-10-19 の結果(`runs/e1-pub`、4,320 run、1回)

判定器の hash は結果を見る前に `runs/e1-pub.FREEZE.txt` に記録し、集計時に一致した。

| 条件 | 根本原因の検出(12中) | witness 単位(74中) | 1 run の時間の中央値 |
|---|---|---|---|
| A0 | 4(F01, F08 × 2実装) | 42 | 2.43 s |
| A1 | 6(+ F05 × 2) | 46 | 2.39 s |
| B | 8(+ F03 × 2) | 62 | 3.49 s |
| C0 | 8 | 62 | 3.77 s |
| C | 10(+ F10 × 2) | 68 | 4.04 s |

- 誤った VIOLATED・誤った SATISFIED・A1 と低層の矛盾・条件間の挙動の差は0件。検出はすべて3 seed で安定。F12 はどの条件も検出しない。
- B が A1 に足したのは、F03(読んでから拒否)と F05 の拒否された試行(witness 単位のみ。F05 の根本原因は A1 も成立した接続で捉える)。C が B に足したのは F10。
- block 対応の時間比の中央値は B/A1 1.43、C/B 1.18(4並列の負荷下、起動が支配的)。
- 試し走りで、2系統目の非ブロッキング connect に対する B の誤った SATISFIED が見つかり、adapter を直した(DESIGN.md 付録 A12)。
- 違反でない property の INCONCLUSIVE が2件。並列に動く他 worker の netns を `ip` が開いた strace の行を parser が読めなかったもの [R]。

## F11をVMなしで検証（2026-10-20）

`f11_local.py` で、同じF11ハンドラを通信なしで実行した。公開8ケース×V/R/H×4条件×3反復の288実行で照合エラー0件。違反版の対象3ケースだけをGDBとMemcheckが毎回検出し、native・C0・Cの全216応答が契約に合った。Memcheckの記録からMukoz形式のVIOLATEDまで確認した。詳細・記録・再実行手順は [F11.md](F11.md)。既存のHTTP/OS観測実験との統合比較と、Cの採用判断は保留である。

記録ベースの試験を行う前に、`python3 ../restore_evidence.py` で [保存済み記録](../evidence/README.md) を復元する。
