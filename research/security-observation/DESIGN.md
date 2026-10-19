# Mukoz — セキュリティ観測階層の比較実験設計書

- 文書版: 0.1-design-draft.13
- 作成日: 2026-10-19(draft.1)/ 改訂: 2026-10-19(draft.2, draft.3, draft.4, draft.5, draft.6, draft.7, draft.8, draft.9, draft.10, draft.11, draft.12, draft.13)
- 実験ID: SEC-OBS-01
- 状態: 実装・実験のための設計案。実装、実行、性能測定は未実施。数値は試験規模・予算・採用基準の提案であり、測定結果ではない。
- 関連設計: Mukoz 0.1.0 の `docs/`(設計 revision 0.5)。[I1]
- 照合範囲: 2026-10-19 に `docs/` の該当章(01, 02, 05, 06, 07, 09, 10, 12)を照合した。`src/` は判定・採否・native-process・隔離・policy の該当箇所だけを読んだ(commit f018494。付録A)。ビルド・テストは再実行していない。ホストでの実測と公開資料の確認も付録Aに記録する。
- 対象読者: Mukozの設計者、実装エージェント、実験担当者、評価結果のレビュー担当者。

本実験の目的は「低レベルほど優れている」と証明することではない。API・外部状態だけの検証に対し、OS操作の観測とバイナリ実行時の計装を追加すると、どの契約違反が新たに検出でき、何が依然として判定できず、そのためにどれだけの費用を要するかを明らかにすることである。

製品に戻す問いは一つに絞る。**現行 Mukoz の `native-process` は `effects.no_forbidden` と `machine.memory.access` を観測できず `NOT_EVALUATED` を返す。外部の観測器(strace、Memcheck)を capability として足すと、その未評価をどこまで正しく埋められるか。**

## 変更履歴

| 版 | 変更 |
|---|---|
| draft.3 | 事前調査(付録A)を反映。正解ラベル用追跡器を「有無は VFS 層、byte 数は syscall 境界の別機構」の二層に変更(7.2, 19, 21)。現行 `native-process` の隔離は HTTP サービスに流用できないと確定し、SUT sandbox を別に作る前提へ変更(8.2)。必要なツールがこのホストに無いことを記録し WP0 の判断事項に追加(8.1, 17)。atime・Rust の syscall 列の観測結果を反映(9.1, 9.2)。Valgrind の子プロセス追跡と出力分離を core manual に基づいて規定(9.6)。Landlock の ABI 範囲を限定(14.1)。判定部への外部 claim の合流点と、capability 照合が未実装であることを反映(16.1, 16.2, 17)。参考資料を訂正・追加(22) |
| draft.4 | 専用 VM `sec-obs-01` を用意し、版と隔離を照合した結果を記録(8.1、付録 A4)。資源割当を実際の構成に合わせた。カーネルは 6.12 のままとし、byte 数の層を VFS 関数に掛けた bpftrace へ移した(7.2, 8.1, 19) |
| draft.5 | E0 第1段(観測器の校正)を専用 VM で実施し、結果を `e0/README.md` と付録 A5 に記録。名前空間越しの追跡(8.2)、pid 名前空間による子の強制終了(8.2)、Valgrind の XML とテキストログ(9.6)、校正の実施範囲(11.1)を反映 |
| draft.6 | WP0.7 の SUT sandbox を作り、受入試験を通した(`sbx/README.md`、付録 A6)。pid 1 の扱いを「待つだけの init + 事前登録する猶予」に決定(8.2)。cgroup は VM 上で root が作る方式にした(8.2) |
| draft.7 | WP0.5 を「Mukoz 側の合流(WP0.5a)」と「ハーネス側の変換と 7.5 の負の試験(WP0.5b)」に分けた。WP0.5a を branch `wp05-external-claims` に実装(未 merge)。入口は `mukoz check --external-claims <file>`(実験的)、イベントから claim への変換はハーネス側(2026-10-19 の決定)(16.1, 17) |
| draft.8 | WP0.5b のハーネス側の変換器を作り、人工イベントで 7.5 の負の試験と、実際の `mukoz check --external-claims` を通した(`adapter/README.md`、付録 A7)(16.1, 17) |
| draft.9 | WP3 第1段: strace の記録を正規化イベントにする parser(FD 系譜、SUT の範囲、pid 名前空間の対応付け、receipt)を作り、E0・WP0.7 の実際の trace 60 本で校正した(`normalize/README.md`、付録 A8)。B は SEC-OBS-01 の間ハーネス側に置き、`--external-claims` で合流させると決めた(9.2, 16.4, 17) |
| draft.10 | WP1: Pilot 6シナリオの契約・V/R/H・公開8ケース・事前ラベルを作り、SUT の外の観測(fanotify・VFS bpftrace・SYN・受信器・gdb の watchpoint)で 144 run がラベルどおりだった(`fx/README.md`、付録 A9)。常駐する SUT の停止を sandbox に足した。strace を付けた 96 run を B に通し、判定がすべてラベルどおりだった(17) |
| draft.11 | WP4: Memcheck の XML を正規化イベントにする parser と、論理領域の property を常に NOT_EVALUATED にする述語を作った。Pilot の 18 artifact を Nulgrind と Memcheck の下で各 144 run 動かし、挙動・判定がすべて期待どおりだった。F12 は保留された(`fx/README.md`、`normalize/README.md`、付録 A10)(9.6, 17) |
| draft.12 | WP2: A0・A1 の判定器を作り、tracer なしの 144 run と relatime の補助 24 run で、同じ run の正解に対して誤った違反・誤った成功が0件だった。relatime の fixture は utime が ctime を進めることに注意が要ると分かった(9.1, 17、`fx/README.md`、付録 A11) |
| draft.13 | E1 の形を公開ケースで実行した(WP5 の一部)。2系統目の実装(C、同じ作者)、反復 seed、条件順の均衡、block の並列実行、集計器を作り、6シナリオ × 2実装 × V/R/H × 公開8ケース × 3 seed × 5条件 = 4,320 run を流した。2系統目の実装で、非ブロッキング connect を「禁止先へ接続していない」とする誤った成功が B に見つかり、保留へ直した。保留ケースが無いので主比較ではない(`fx/README.md`、付録 A12)(11.2, 13.1, 17) |
| draft.2 | 関連設計を廃止済みの 0.4 単一ファイルから現行 `docs/`(revision 0.5)へ差し替え。製品対象を「AArch64/Apple Silicon 初期」から現行の独立軸・Tier 1 `linux-x86_64` に修正(1, 2, 8, 17章)。既存 Executor との対応表を追加(2.3)。API を現行コマンドへ修正(16.4)。判定器を新規実装せず既存の claim 軸・採否を再利用する構成へ変更(16章)。X3 を既存 emulated executor(icicle-emu 既定)上の実験に変更(15.3)。Pilot を公開ケースに限定(12.1)。ファイル系正解ラベルの追跡器を B と別系統に限定(7.2)。`sendfile`/`copy_file_range`/`splice` を B の必須観測へ追加(9.2)。atime の扱いを事前登録(9.1, 12.2)。判定順を既存 6.6 に合わせた(10.2)。メモリ系の実装割当と C 採用基準の厳しさを明記(6.1, 18.2) |
| draft.1 | 初版 |

## 目次

1. 結論として決めたいこと
2. 既存Mukoz設計との関係
3. 研究上の問いと仮説
4. 比較条件
5. 実験全体の構成
6. 検証対象とシナリオ
7. 契約・正解・評価器の独立性
8. 試験環境と安全境界
9. 観測・対応付けの設計
10. 判定モデルと証跡
11. 固定入力比較の実行手順
12. 試験数・予算・段階化
13. 指標と統計解析
14. サンドボックスによる問題の隠蔽を調べる副実験
15. 入力探索・診断ビルド・ARM64ルーチンの拡張実験
16. Mukozへの実装構成
17. 作業パッケージと完了条件
18. 採用・保留・中止の判断基準
19. 妥当性の限界と解釈禁止事項
20. 結果レポートの形式
21. 実験開始前チェックリスト
22. 参考資料
- 付録A. 事前調査の記録(2026-10-19)

## 1. 結論として決めたいこと

本実験の成果は、検出件数のランキングではなく、次の設計判断である。

| 判断 | 実験で必要な根拠 |
|---|---|
| OS観測を `native-process` の追加 capability にするか | 強い外部観測ベースラインでも見えない契約違反を、再現可能な証拠で追加検出できること |
| メモリ計装を標準実行に含めるか、任意の診断にするか | OS観測に対する追加検出、対応範囲、観測の攪乱、時間・メモリ費用 |
| どの契約にはどの証拠が必要か | 契約別の観測可能性と、観測不能を正しく保留する能力 |
| 既存ツールのラッパー以上の価値があるか | 契約との対応付け、未確認の管理、反例再生、正しい許可動作の受容に実用上の差があること |
| 既存 Mukoz の Executor・capability・判定へ何を戻すか | OS依存の収集器と、共通化できる契約・証拠要求・判定・証跡の分離。現行の `NOT_EVALUATED (REQUIRED_CAPABILITY_UNAVAILABLE)` を正しく埋められる範囲 |

「低レベル」を一括りにしない。

- 成果物の段階: 配布する実行ファイルを試験すること。
- 観測の位置: API、外部状態、OS操作、メモリアクセスなどのどこを見るか。
- 実行方式: ネイティブ実行、動的バイナリ計装、CPUエミュレーションのどれを使うか。

A条件も完成した実行ファイルを試験する。したがって、AとBの比較はUnit Test対Binary Testではなく、同じ実行成果物に対する観測能力の比較である。

背景のLSCBenchは、機能試験とセキュリティ試験を分けてAI生成コードを評価する研究である。本実験ではその問題設定を参照するが、論文のモデル順位や成功率の再現は行わず、観測方式の寄与だけを切り分ける。[S1]

## 2. 既存Mukoz設計との関係

### 2.1 維持する方針

既存設計から、次を維持する。[I1]

| 項目 | 本実験での扱い |
|---|---|
| 製品の中心 | Binary Test Tool。一般的な脆弱性診断製品へ変更しない |
| 検証の入力 | Artifact、Contract、Binding、Suite、Policy(trial zone を含む) |
| 正しさの根拠 | 生成器の自己説明ではなく、独立に承認された契約 |
| プラットフォーム | ISA・format・ABI・OS・host を独立の軸として扱う(`docs/02` 2.1)。特定環境を前提にしない。Tier 1 は現時点で `linux-x86_64` のみ(`docs/09` 9.8)。macOS arm64・Linux arm64・Windows は P5 |
| FSL | 契約を供給する一つのフロントエンド。実験の必須依存ではない |
| 判定 | SATISFIED_IN_SCOPE / VIOLATED / INCONCLUSIVE / NOT_EVALUATED(`docs/06` 6.4) |
| 採否 | ACCEPT_WITHIN_SCOPE / HOLD / REJECT。`release_authorized` は常に false(`docs/06` 6.6) |
| 未確認 | 観測不能・未対応・欠測・予算超過を成功へ変換しない(`docs/01` P5) |
| API | 出力 envelope `mukoz/1` と現行コマンド(`docs/07` 7.2)を継承する。新しい例示スキーマを既存実装と同一視しない |

### 2.2 本書で追加する範囲

比較しやすい既存観測器(strace、Valgrind)を利用するため、研究ハーネスを設ける。**第一環境は現行 Tier 1 と同じ `linux-x86_64`(Debian 13)とする。**理由は次のとおり。

- 現行の `native-process` は x86-64 Linux の対象を x86-64 Linux ホストで実行するものに限られる(`docs/12` 12.6)。A1 を現行 `native-process` の観測と直接対応させられる。
- `linux-aarch64` はまだ一度も試されていない(`docs/02` 2.4)。最初の実験に未検証のホストを持ち込むと、観測方式の寄与とホスト差を切り分けられない。
- strace と Valgrind は amd64/Linux を対応対象に含む。[S3][S6]

AArch64 Linux での追試は、主比較の後に別の環境版として行う(5章 X5)。Linuxで得たファイル・ネットワーク・プロセス観測の結果を、そのままmacOSやWindowsに適用してはならない。他ホストへ戻せるものは、まず契約の表現、証拠要求、判定、反例管理である。収集器の可用性と権限はホストごとに検証する。

既存設計はCPUエミュレーションとネイティブ実行を分けているため、本書でも混在させない。既存のemulated executorへ直接戻せる比較は、第15.3節の別実験とする。

本書のモジュール、データ型、collector の組込み方は実装提案である。現行 `src/` に同名の要素があることは主張しない。

### 2.3 既存Executorとの対応

主比較の条件と、現行 Mukoz の Executor の観測能力(`docs/06` 6.3、`docs/12` 12.6)を対応させる。

| 本書の条件 | 対応する現行 Mukoz | 現行での観測 | 本実験で足すもの |
|---|---|---|---|
| A0/A1 | `native-process` | stdout/stderr、終了状態、宣言ファイル。`effects.no_forbidden`・`machine.memory.access` は NOT_EVALUATED | A1 は HTTP 応答・DB・受信器を外部観測として追加 |
| B | (なし) | — | strace による syscall 観測を `native-process` の capability として追加する案 |
| C0/C | (なし) | — | Valgrind 下の実行を別 Executor 相当として追加する案 |
| (参照のみ) | emulated + effect model(`linux-stdio/1` 等) | syscall 試行とすべてのメモリアクセスを観測・強制 | 主比較には入れない(下記) |

emulated + effect model を主比較に入れない理由: 現行のプロセス対象は static ET_EXEC(または static-pie を含む native 側)に限られ、PT_INTERP は `UNRESOLVED_DEPENDENCY`、表外の syscall は `UNSUPPORTED_DURING_RUN` になる(`docs/12` 12.2)。本実験のSUT(libc とソケットを使うHTTPサービス)はこの範囲外である。代わりに、emulated の強みである途中メモリアクセスの観測は第15.3節(X3)で小さなルーチンに対して比較する。

## 3. 研究上の問いと仮説

### 3.1 主要仮説

| ID | 仮説 | 支持する観測 | 支持しない観測 |
|---|---|---|---|
| H1 | OS観測は、外部応答に現れない禁止作用の検出を増やす | A1で未検出の違反をBで再現可能に検出 | A1で同じ違反をすべて検出、またはBの追加検出が誤検知 |
| H2 | メモリ計装は、OS操作にも現れないメモリ違反の検出を増やす | Bで未検出の対応範囲内の違反をCで検出 | Cで増えない、または計装器の不具合・意味変化でしか再現しない |
| H3 | 観測を低くするだけでは業務上の意味を補えない | IDOR等ではA1が十分、論理的メモリ領域等ではCでも判定不能 | 別の汎用的な観測から、追加仮定なしに必要な意味を回復できる |
| H4 | 計装は検出力だけでなく実行を変える | C0/Cで時間、分岐、終了、違反発生率が変化 | 検証範囲では差を確認できない。ただし同値性の証明とはしない |
| H5 | 検出と隔離を混ぜるとアプリの欠陥を見逃す | 強制制御ありで外部被害が消えても、禁止試行または脆弱性が残る | アプリ自身が適切に拒否し、強制制御の有無で結果が変わらない |

### 3.2 最初の実験で答えないこと

AI生成コード全体の脆弱性率、モデル間の優劣、未知の全脆弱性への検出率、形式的な安全性、商用需要の大きさは推定しない。

固定入力で検出数が増えても、入力探索が改善したとはしない。逆に、カバレッジが増えても、契約違反の検出が増えなければセキュリティ上の追加効果とは数えない。

## 4. 比較条件

### 4.1 主比較の5条件

| 条件 | 観測できる情報 | 実行方式 | 位置づけ |
|---|---|---|---|
| A0: API | HTTP status/body/header、呼出し成功・失敗、SUT終了・生存 | ネイティブ | 最小の外部応答検証 |
| A1: External | A0＋SUTのstdout/stderr、最終ファイル・DB状態、検査用通信先の受信内容 | ネイティブ | 実務的に強いブラックボックス比較対象。現行 `native-process` の観測に外部状態を足したもの |
| B: OS | A1＋対象プロセス木のファイル・接続・プロセス操作と戻り値 | ネイティブ＋strace | OS観測の追加価値 |
| C0: Runtime control | A1＋B相当のOS観測。メモリエラー検出は無効 | Valgrind Nulgrind＋strace | 計装実行基盤自体の影響を調べる対照 |
| C: Memory | C0相当の外部・OS観測＋Memcheckのメモリエラー情報 | Valgrind Memcheck＋strace | バイナリ実行時のメモリ検査の追加価値 |

主な比較は B−A1 と C−B である。A0−A1は外部観測を充実させた効果、C−C0は同じ系統の実行基盤でメモリ検査を有効にした差として扱う。

A1を必須にする理由は、通信受信器やDBの最終状態を調べれば発見できる問題まで、「低レベルでしか発見できない」と誤認しないためである。

Cは「すべての低レベル解析」の代表ではなく、Memcheckという特定の検査器を用いる比較条件である。Memcheckは不正なメモリアクセス、解放後の使用、不正な解放などを検出するが、アプリ固有の秘密領域や認可を自動的に理解する検査器ではない。[S4]

### 4.2 同一成果物と同一実行の区別

A0/A1/B/C0/Cには同じ実行ファイルのbytesを渡す。通常のバイナリを、条件ごとに再コンパイルしない。依存ライブラリ、設定、起動引数も固定する。

ただしC0/Cでは、ディスク上の対象成果物は同一でも、実行基盤、メモリ配置、スケジューリング等が変わる。同一digestは、ネイティブ実行との意味的同値を保証しない。

Nulgrindは動的計装基盤を使うが追加の検査を行わない対照として利用できる。ただしMemcheckとのメモリ管理や実行挙動の差を完全に相殺する対照ではない。[S5]

### 4.3 比較を汚染しない規則

1. 全条件に同じ契約・許可範囲・入力・初期状態を与える。B/Cだけに脆弱性の正解や追加ポリシーを教えない。
2. 主比較では入力生成・変異・適応的な再試行を停止する。
3. Memcheckの出力はSUTのstderrと別チャネルに保存し、A0/A1へ漏らさない。
4. 正解ラベル、修正差分、診断用ソース、評価専用の観測は検査器の入力にしない。
5. Cで取得した情報を隠す比較と、Cなしで実行する比較は別実験として扱う。
6. 計装器の起動処理を含む生ログを保存する。都合の悪いイベントを一括で無視しない。

### 4.4 代替技術の位置づけ

straceはLinuxのシステムコール、引数、戻り値を再コンパイルなしに追跡できるため、最初の観測器とする。[S2][S3]

Tetragon/eBPFは、Bで観測の追加価値が確認できた後、収集費用・対応範囲を比較する追加バックエンド候補とする。初回から専用eBPF基盤を作ることは出荷条件にしない。[S8]

Frida StalkerやAFL++は制御フロー・カバレッジを利用した探索の追加実験に回す。制御フローを取得できることと、メモリ安全性を判定できることは異なる。[S10][S11]

AddressSanitizerはコンパイラ計装とランタイムを使うため、診断ビルドを別途作る副実験とする。同一成果物のC条件として混ぜない。[S7]

## 5. 実験全体の構成

| 実験 | 内容 | 必須度 |
|---|---|---|
| E0 | 観測器・正解ラベル・判定器の校正 | 必須 |
| E1 | 固定入力をA0/A1/B/C0/Cで別々に実行 | 必須・主比較 |
| E2 | 同一実行の証拠をマスクして再判定 | 必須・観測情報の純増分を調べる |
| E3 | アプリ検査と強制隔離を分離した対照 | 必須・小規模 |
| X1 | 同一時間／同一実行数での入力探索比較 | E1後の任意拡張 |
| X2 | ASan診断ビルドとの比較 | 任意拡張 |
| X3 | 既存emulated executorでの最終状態対途中アクセス観測 | 既存設計へ戻す任意拡張 |
| X4 | 実在プロジェクトの修正前後で外的妥当性を確認 | 製品採用の一般化前に実施 |
| X5 | AArch64 Linux ホストでの E1 部分追試 | `linux-aarch64` を対象に含める前に実施 |

最初の実装はE0と小規模E1から始める。X1〜X5を先に実装して主比較の完成を遅らせない。

## 6. 検証対象とシナリオ

### 6.1 検査用アプリケーション

中心対象は、試験専用のファイル／文書取得サービスとする。共有機能はファイル取得、設定出力、補助的な取り込み、認可付き文書取得である。各シナリオは必要な機能だけを有効にした小さなfixtureに分ける。

CAPH TECHや第三者の本番環境、実際の顧客データ、実際の資格情報は使わない。攻撃結果はすべて合成データ、検査用ファイル、ローカルの受信器、無害な補助実行ファイルで観測する。

実装の割当:

- F01〜F09: Rust主体の実装と、別の実装経路を持つC/C++等の実装の2系統。
- F10〜F12(メモリ系): 2系統とも native C/C++ とする。案として、一方は C の `malloc`/`free` と生ポインタ、他方は C++ の `new`/`delete` とコンテナの生データアクセスを使う。Rust の `unsafe` で同等の欠陥を作る系統は主比較に入れない(言語差と欠陥の作り方が交絡するため)。
- 具体的な言語割当・依存ライブラリはE0で確定し、本試験前に固定する。

二つの実装は別のコード経路を持たせるが、同じ作者・同じ契約から作る可能性があるため、統計的に完全に独立とはみなさない。

### 6.2 シナリオ一覧

以下の「見える可能性」は実験前の仮説であり、結果ではない。

| ID | 対象となる違反 | 契約の例 | 見える可能性／狙い |
|---|---|---|---|
| F01 | パス解釈により許可root外の内容を返す | 許可されたresourceだけ応答可能 | A0で検出できる基本対照 |
| F02 | 固定シンボリックリンク経由で保護対象を返す | 表記上のpathではなく解決先resourceで制限 | A0とBの対象同定の差 |
| F03 | 保護ファイルを読み、その後403を返す | 保護対象からの正のbyte読取り自体を禁止 | A1では atime 設定次第(9.1)。Bの追加効果候補 |
| F04 | 保護ファイルへ一時書込みし、内容を元に戻す | 最終内容一致ではなく、書込み成功自体を禁止 | metadata等でA1でも検出できる可能性を含め、途中の作用との違いを比較 |
| F05 | 禁止先へ接続し、その後応答を拒否する | 禁止先への接続試行／接続成立を別々に制限 | 成立はA1の受信器でも検出可能。B独占とはしない |
| F06 | 入力処理により不許可の補助プログラムを起動する | 許可された補助実行ファイル以外のexec成功を禁止 | ファイル等の痕跡を残さない場合のOS観測 |
| F07 | 秘密属性の設定値を応答またはログへ出す | 出力チャネルごとに開示可能な属性を定義 | A0/A1が強い対照。単なるopen観測では不足 |
| F08 | 他tenantの文書を返すIDOR | trusted policy上のprincipal-resource関係を満たす | API＋権限の意味が重要。B/Cに有利とは限らない |
| F09 | 再送により業務状態を二重に更新する | idempotency keyに対する更新は一回 | A1のDB／状態観測が重要 |
| F10 | ヒープ境界外を読み、応答は正常のまま | 対応するallocatorの有効領域外へアクセスしない | Cの追加効果候補。実害の成立とは区別 |
| F11 | 解放後のメモリを使用し、正常応答する | freeされた領域にアクセスしない | Cの追加効果候補。nativeでの症状は別評価 |
| F12 | 一つの有効なallocation内の論理的秘密領域を読む | 呼出しに許された領域以外の読取りは禁止 | Memcheckでも判定できない可能性を調べる限界対照 |

F12は、一般的なallocator単位のメモリ安全性と、契約で定める情報アクセス制限を区別するために置く。「Cでエラーが出なかったから安全」と結論してはいけない。第15.3節では明示的な領域Bindingを持つ小さなルーチンで、この差を既存の emulated executor 上で検査する。

### 6.3 各シナリオの3種類の成果物

各実装について次の3種類を作り、すべて同じ正常系契約を満たすことを事前確認する。

| 種類 | 内容 | 目的 |
|---|---|---|
| V | 一つの主たる契約違反を含む成果物 | 既知の違反が検出できるか |
| R | その違反を修正した成果物 | 修正後を誤って拒否しないか |
| H | 危険に見えるが、明示的に許可された処理を行う成果物 | 意味を無視した過剰検出を測るhard negative |

Hの例は、許可された補助プログラムの起動、許可された受信器への接続、scratch領域への一時書込み、認可済みtenantへのアクセスである。V/R/Hで契約そのものは変えず、同じ契約に含まれる許可範囲をHが使う。

Vの不具合が正常系を壊していたら、セキュリティだけの比較とはいえない。正常系不成立として除外理由を記録し、同じルールで作り直す。修正後に別の違反が見つかった場合も、Rを自動的に安全ラベルのままにしない。

### 6.4 固定入力スイート

本試験は各シナリオにつき16ケースとする。

- 正常系4ケース。
- 境界に近いが許可される入力・操作4ケース。
- 禁止経路を試す入力・操作8ケース。

入力にはHTTP requestだけでなく、tenant、ファイル配置、symlinkの固定配置、seed、複数requestの順序を含む。F09のような状態を持つ検証は一連のrequest全体を一ケースとする。

V/R/Hに同じ16ケースを与える。禁止経路を試す入力でも、R/Hでは安全に拒否される、または契約内の許可動作だけになることが期待される。

ケースの半分は校正用に公開し、残りはパーサやルールの調整から分けた保留セットとする。それぞれ正常2、許可境界2、禁止経路4を含む。すべて本試験前に固定し、失敗結果を見て入力を後付け追加しない。追加した入力は次の実験版に所属させる。

**保留8ケースは、E0・Pilot・ルール調整のいずれでも実行しない。**本試験で初めて実行する。

**主要な検出率・採用基準の評価には保留8ケースだけを使用する。**公開8ケースは回帰・校正結果として別表にし、16ケース合算は補助結果と明記する。反例の仕組みを説明する際は公開ケースも使ってよいが、未見入力への成績に混ぜない。

## 7. 契約・正解・評価器の独立性

### 7.1 三つの役割を分離する

```text
契約・正解作成側                 試験実行側                    集計・裁定側
  policyを定義                   opaqueなartifactを実行         正解とfindingを照合
  V/R/Hを用意                    許された観測のみ収集           真偽・対応範囲を裁定
  witnessを確認                  契約を決定論的に評価           条件別の追加効果を計算
```

初期は同じ担当者が複数の役割を担ってもよいが、ディレクトリ、権限、手順、レビュー時点は分離する。実験担当AIがVのラベルを見て、そのケース専用の検査条件を生成することは禁止する。

LLMによるfixture・入力・Bindingの下書きは許容する。ただし本試験中は固定済みの成果物と決定論的評価器だけを用い、LLMの評点を合否にしない。

### 7.2 「正解実装」と「実際の違反」を区別する

ソースに不具合を挿入しただけでは正例にしない。最適化や環境によって経路が消えている可能性があるため、各V × 入力 × 環境について、違反に到達するwitnessを確認する。

- ファイル系は、**二層の追跡**を組み合わせる。**strace自身の出力だけを正解にしない。**
  - 有無の層: fanotify(VFS 層の fsnotify フック)で、保護対象へのアクセス・変更の有無を記録する。ptrace とも syscall 境界とも別の観測点である [R]。ただし通常のイベントに byte 数は無く、mmap 経由のアクセスは報告されず、連続イベントはまとめられ、キューあふれでは捨てられる(FAN_Q_OVERFLOW)。[A2]
  - byte 数の層: bpftrace の kretprobe を VFS の関数(`vfs_read`・`vfs_write`・`vfs_readv`・`vfs_writev`・`vfs_copy_file_range`・`do_splice`・`do_splice_direct`・`do_sendfile` など)に掛け、戻り値の byte 数を数える。syscall 境界より内側を見るので、strace とは観測点が違う。それでも同じ syscall から呼ばれるので、独立しているのは一部だけである [R](外れる条件: SUT が mmap で読み書きする場合。そのときはどちらの層にも byte 数が出ない)。
    - 専用 VM(6.12)では上の関数すべてに kprobe を掛けられた。1 MiB の tmpfs ファイルで1回試した。`cat` の `vfs_read` の合計は 1,054,084 byte で、ファイルの大きさより 5,508 byte 多かった。`cp` では `vfs_copy_file_range` が 1,048,576 byte を返し、その内側で `do_splice_direct` も1回呼ばれた。[A4]
    - したがって、プロセス名ではなく**対象ファイル(inode)で絞って数える**。入れ子になる関数(`vfs_copy_file_range` とその内側の splice)は**外側だけを数え**、二重に数えない。数える関数の集合と絞り込み方は事前登録に含める。[R] 5,508 byte の差は、`cat` が共有ライブラリなど対象以外のファイルを読んだ分だと推定している。どのファイルかは確かめていない。
    - 補助として、Linux audit の syscall ルールで read/write 等の戻り値を記録してもよい。ただし audit は strace と同じ syscall 境界を見るので、そこで生じる誤り(未知の syscall、FD の誤帰属など)は共有し得る。
  - fanotify で byte 範囲が取れる FAN_PRE_ACCESS は Linux 6.14 以降にしかなく、専用 VM のカーネル 6.12 では使えない。カーネルは上げない(8.1)。[A2][A3]
  - 二層の結果と B の結果が一致しない場合は、どれも正としないで裁定対象に残す。fixture固有の観測と手動の状態遷移確認も併用する。
- 通信系は、隔離された受信器のaccept／受信内容を確認する。接続試行の正解は受信器ではなく、Bと別の機構の追跡(audit の connect 記録等)またはネットワーク名前空間側の記録で確認する。接続試行と成立は別ラベルにする。
- プロセス系は、Bと別の機構の追跡(audit の execve 記録等)と、補助実行ファイル自身が controller 領域へ残す起動記録を併用する。
- audit・bpftrace・fanotify の完全なモードはいずれも root を要する(現行ホストでは非特権の fanotify は `FAN_REPORT_FID` での inode 単位の通知だけが使えた。1回観測。[A3])。追跡器は controller 側の権限で動かし、SUT へ権限を渡さない。
- 業務系は、SUTとは別の単純な参照状態モデルで期待状態を計算する。
- メモリ系は、コード／逆アセンブルによる経路確認と、別系統の診断器または明示的なメモリモデルを併用する。Memcheck自身の出力だけを正解にしない。

別ビルドや評価専用hookはラベルの妥当性を補う資料であり、主比較の検査器へ渡す証拠ではない。元の成果物で対応する動作が存在することを確認できなければ、GROUND_TRUTH_UNRESOLVEDとして主要な精度集計から分ける。

新しい未登録のfindingが出た場合は、false positiveとも新規発見とも即断せず、独立裁定後に追加ラベルを付ける。元のラベル表と裁定履歴は残す。

### 7.3 契約を先に定義する

最低限、以下を機能要件とは別のproperty IDとして持つ。

| Property例 | 判定対象 |
|---|---|
| FUNC.ALLOWED_READ | 許可された文書を正しく返す |
| SEC.FS.NO_FORBIDDEN_OPEN | 禁止対象の読取り可能handleを取得しない |
| SEC.FS.NO_FORBIDDEN_READ | 禁止対象から正のbyte数を読み取らない |
| SEC.FS.NO_FORBIDDEN_WRITE | 保護対象への正のbyte書込みを成立させない |
| SEC.NET.NO_FORBIDDEN_ATTEMPT | 禁止宛先への接続を試行しない |
| SEC.NET.NO_FORBIDDEN_CONNECT | 禁止宛先との接続を成立させない |
| SEC.PROC.ALLOWED_EXEC_ONLY | 認可された補助実行ファイルだけを起動する |
| SEC.DATA.NO_SECRET_OUTPUT | 指定チャネルへ指定秘密情報を出さない |
| SEC.AUTH.RESOURCE_ACCESS | trusted policyの認可関係を満たす |
| BUS.IDEMPOTENCY | 指定した再送系列で重複更新しない |
| MEM.HEAP.ADDRESSABLE | 対応するヒープ検査範囲で有効な領域へアクセスする |
| MEM.REGION.READ_AUTHORIZED | 契約が許可する論理領域だけを読む |

openの成功、正のbyte数を返すread、秘密情報の外部開示は異なる。これらを一つの「漏洩」ラベルにまとめない。

また、403を返す前にファイルを読んだことが常に脆弱性になるわけではない。本実験では、その読取りを禁止する契約があるケースに限って違反とする。

既存 Mukoz の property との対応: `SEC.FS.*`・`SEC.NET.*`・`SEC.PROC.*` は現行 `effects.no_forbidden` を作用の種類と段階(試行／成立)で細分したもの、`MEM.REGION.READ_AUTHORIZED` は現行 `machine.memory.access` の `allowed_read`(`docs/06` 6.2)に対応する。製品へ戻す際は、新しい名前空間を作る前に既存 property の細分として表現できるかを検討する。

### 7.4 契約とBindingの例

以下は新しい交換形式の設計例であり、既存FSL／Mukozが受け付ける構文ではない(現行の Contract/Binding は TOML。`docs/04`、`docs/12` 12.2)。式は将来の型付きIRへ変換する想定の説明用表現である。

```yaml
schema: mukoz-security-observation-experiment/1-draft
contract:
  id: file-service-policy-v1
  properties:
    - id: SEC.FS.NO_FORBIDDEN_READ
      predicate: forbidden_positive_read_absent
      subject: request_process_tree
      resource_selector: denied_resources_for_principal
      required_evidence:
        - successful_read_byte_count
        - resolved_resource_identity
        - subject_attribution
        - monitoring_complete_for_scope
      scope:
        start: first_request_sent
        end: process_tree_terminated_and_collectors_drained
    - id: SEC.DATA.NO_SECRET_OUTPUT
      predicate: canary_absent_in_registered_output_channels
      required_evidence:
        - complete_registered_channel_capture
binding:
  resource_catalog: fixture-manifest.json
  principal_source: controller-signed-case
  access_policy_source: approved-policy.json
  file_identity_mode: frozen_fixture_catalog_plus_fd_lineage
  unsupported_access_paths:
    - file_backed_mmap_data_access
    - io_uring_file_io
    - cross_process_fd_transfer
    - concurrent_path_replacement
```

あるpropertyに必要な証拠がない条件は、そのpropertyを評価できないと返す。A0にOS情報がないからといって、SEC.FS.NO_FORBIDDEN_READを403検査へ勝手に弱めない。これは現行 `native-process` が `effects.no_forbidden` を `NOT_EVALUATED (REQUIRED_CAPABILITY_UNAVAILABLE)` とする扱いと同じである。

### 7.5 評価器自身の負の試験

修正済み／違反あり／許可された例に加え、人工的な証跡を使って次を必須試験にする。

- read > 0をread = -1へ変更したとき、成功読取り違反が消えること。
- 許可principalを禁止principalへ変更したとき、認可propertyの判定が変わること。
- 観測の開始欠落、途中欠落、対象不明、Binding失効で成功を返さないこと。
- 一つの有効な反例の後にログが欠落しても、その反例を消さないこと。
- SUTの出力にPASSや偽のJSONレポートが含まれても、評価器の判定に昇格させないこと。
- 全入力を事前条件外にしても、空の試験範囲を成功にしないこと(現行 `VACUOUS_SCOPE`)。
- `sendfile` の正の戻り値を 0 に変更したとき、成功読取り違反が消えること。

## 8. 試験環境と安全境界

### 8.1 実行環境

第一候補は、**`linux-x86_64`(Debian 13)の専用仮想マシン**とする。検査対象は x86-64 の ELF とし、異種ISAの変換実行と混在させない。現行 Tier 1 ホストと同じ OS 系列にすることで、A1 の結果を現行 `native-process` の観測と比べられる。ただし同じ VM 内でも、作業用ホストそのもの(ソースリポジトリや資格情報があるもの)では実行しない。

AArch64 Linux(Apple Silicon 上の VM 等)は X5 の追試環境とする。主比較の結果を AArch64 や macOS へ転用しない。

ValgrindはAMD64/Linuxを主な開発対象として対応プラットフォームに含めている(ARM64/Linux も対応、ARM64/Darwin は載っていない)。一方、利用するカーネル、命令、libc、バイナリ形式、起動方式で実際に動作することはE0で確認する。[S6]

**現行ホストには strace、valgrind、bpftrace、auditctl が入っていない**(2026-10-19 に確認。[A3])。gcc 14.2(libasan 付き)、rustc 1.97.1/1.99.1、unshare はある。導入には root が要る。**2026-10-19、導入先は専用 VM と決めた**(現行ホストには入れない)。VM の guest kernel・ツールの版は WP0 で固定し、8.1 の記録対象に含める。

**用意した VM**(2026-10-19。管理エージェントが作成し、作業用ホストから ssh で照合した。[A4]): Proxmox の VMID 103 `sec-obs-01`、192.168.68.91。CPU 種別 host・8 vCPU・メモリ 8 GiB・ディスク 80 GiB。guest kernel 6.12.113+deb13-amd64。strace 6.13+ds-1、valgrind 1:3.24.0-3、bpftrace 0.23.2-1、auditd 1:4.0.2-2+deb13u1、gcc/g++ 14.2.0-19、libasan8 14.2.0-19、clang 19、rustc 1.97.1 と 1.99.1(rustup。`~/.cargo/bin` は非対話の ssh では PATH に入らない)。通信は Proxmox 側のファイアウォールで入出力とも既定 DROP、許可は作業用ホスト(192.168.68.76)から 22/tcp への入りだけ。隔離した状態のスナップショット `sec-obs-01-base` を基点にし、試験の回をまたいで状態を持ち越さないときはここへ戻す。

**guest kernel は 6.12 のままとする**(2026-10-19 決定)。理由は三つある。現行 Tier 1 ホストと同じカーネル系列にしておけば、A1 を現行 `native-process` の観測と比べられる。Valgrind 3.24 と strace 6.13 が 7.2 系のカーネルで正しく動くかを確かめていない。byte 数の層は、VFS 関数に掛けた bpftrace で 6.12 のまま取れる(7.2)。trixie-backports の 7.2.6-1~bpo13+1 に上げれば FAN_PRE_ACCESS が使えるが、上げるのは、E0 で VFS の kprobe では足りないと分かったときだけにする。[R] FAN_PRE_ACCESS は、監視側が応答するまでアクセスした側を止める仕組みなので、正解ラベルの観測が SUT の動きを変え得る。対応するファイルシステムが限られるかもしれず、tmpfs で使えるかは確かめていない [U]。

資源割当は上の VM の構成(8 vCPU、8 GiB RAM、80 GiB)とする。依頼時の 16 GiB はホストの空きの都合で 8 GiB に変えた。これは必要最小量の実測値ではない。ログ量や計装器の動作をE0で測り、変更する場合は本試験開始前に環境版を確定する。

記録する環境は、host CPU、VM設定、guest kernel、libc、compiler、最適化flag、依存ライブラリ、strace、Valgrind、collector、evaluator、正解ラベル用追跡器の版とdigestである。latestタグだけでは固定したことにしない。

### 8.2 外側の安全境界

すべての条件で次を共通にする。

| 項目 | 要件 |
|---|---|
| ネットワーク | 外部インターネット、hostのサービス、実LANへの経路を与えない |
| ファイル | hostのhome、SSH鍵、クラウド資格情報、ソースリポジトリをmountしない |
| データ | 合成canaryと試験専用resourceのみ |
| 権限 | SUTは非root。計測に必要な権限はcollector側へ限定 |
| 永続性 | 各ケースのoverlay／volume／DBを再生成し、終了後に破棄 |
| 子孫 | プロセス木だけでなくcgroup等の管理単位で残存を確認・停止 |
| 証跡 | SUTが書けないcontroller領域に保存。SUTのログは未信頼データ |
| 予算 | CPU、memory、wall time、process数、出力量を制限 |

**現行 `native-process` の隔離は、本実験の SUT sandbox にそのままは流用できない**(`src/` を読んで確認。付録A1)。隔離の作り(sealed memfd からの `execveat`、chroot、user/network/pid 名前空間、rlimit、隔離に失敗したら起動しない)は参考にするが、SUT sandbox は別に作る。流用を止める主な箇所:

- 動的リンクの実行ファイルを拒否する(`src/image.rs:248-252`)。chroot に ld.so/libc を置く仕組みが無い(`src/nproc.rs:208-237`)。
- network 名前空間を作った後に何も設定しない(`src/host.rs:109-111`)。親が子の名前空間へ veth を入れる同期点も無い。
- 入力は stdin だけで、「起動してからリクエストを送り、止める」段階が無い(`src/host.rs:208-233`)。
- chroot の中身は宣言ファイルのバイト列だけで、補助実行ファイルを置けない。/proc・/dev を mount しない。
- NOFILE と NPROC が 64 で固定(`src/host.rs:31`)。

非特権の `unshare -Urn` は現行ホストで使えた。ただし lo は DOWN のままで、試験用ネットワークは自分で設定する必要がある。cgroup v2 は `user@1000.service` の下に cpu・memory・pids が委譲されているが、今のプロセスをそこへ移す方法(`systemd-run --user --scope` 等)は試していない [U]。いずれも1回観測。[A3] 専用 VM では controller が root なので、case ごとの cgroup を `/sys/fs/cgroup/sec-obs/` の下に直接作る方式にした(WP0.7。[A6])。この [U] は VM の構成では問題にならない。

外側の strace は user・pid・net 名前空間を越えて SUT を追えた。Valgrind も名前空間の中で動いた(E0、2回。[A5])。ただし strace の行頭の pid は strace 側の名前空間の値で、`clone` の戻り値は SUT 側の値なので、戻り値で親子を結ばない。**pid 名前空間の中では、SUT 本体が終わると切り離した子が SIGKILL された**(E0、2回)。これは隔離が SUT の振る舞いを変える例である。sandbox では SUT を名前空間の pid 1 にしない(間に待つだけの init を置く)か、観測期間の終わり方を事前登録する。**WP0.7 では、待つだけの init(`sbxinit`)を pid 1 に置くことにした。** SUT 本体が終わった後、事前登録した猶予 `grace-ms` の間は子孫を待つ。観測期間は「本体の終了 + grace-ms」で終わり、残った子孫はそこで終了させる。grace 1000 ms では遅れて読む子を三層とも記録し、grace 0 では読む前に終了させた(2回。[A6])。

VM境界を設けても、カーネル脆弱性や隔離突破が不可能とはしない。本試験のSUTは、既知の限定されたアプリケーションレベルの欠陥を持つ自作fixtureに限る。隔離回避、永続化、host攻撃を行うfixtureは含めない。

必要な隔離能力が確認できなければ、実行をblockedにする。ユーザーの通常環境へ自動fallbackしない。

### 8.3 アプリ自体を検査する環境

**主比較では、禁止対象の合成ファイルや検査用受信器を、OS権限上はアクセス可能にする。**アプリの契約によってだけ禁止される操作を観測するためである。

このときも外側のVM・ネットワーク隔離は維持する。「アプリが禁止すべきことをOSにも禁止させない」は、実環境を無防備にするという意味ではない。

強制制御ありの環境は、第14章で別条件にする。

### 8.4 試験トポロジー

```text
Host / 実験controller
  └─ 専用 linux-x86_64 VM（外部経路なし）
       ├─ supervisor / evidence writer
       ├─ collector（SUTから書換え不可）
       ├─ 正解ラベル用追跡器（collectorと別系統、検査側から読めない）
       ├─ 試験用ネットワーク
       │    ├─ 許可された受信器
       │    └─ 禁止扱いの受信器（合成データだけ）
       └─ ケース固有のSUT sandbox / cgroup
            ├─ 同一digestのSUT
            ├─ allow-root / protected-fixtures
            └─ ケース固有DB / scratch
```

準備と評価のためのcontrollerアクセスが、SUTのファイル読取り・通信として数えられない構成を必須にする。

## 9. 観測・対応付けの設計

### 9.1 A0/A1の範囲

A0はclient側で応答の全byte列、status、headerを取得する。SUTがokと報告したことを成功条件にはせず、参照値と比較する。

A1は追加でSUTのstdout/stderr、終了後のファイルmanifest、DBの必要な状態、試験用受信器への通信を取得する。通信の秘密情報検査は、受信器で復号・確認できる試験プロトコル、または平文の隔離ネットワークに限定する。

ファイルmanifestは内容だけでなく、取得可能なsize、mode、mtime、ctime、atime等も含め、時刻分解能を記録する。metadataの差から契約違反を正当に検出できた場合はA1の実績にする。F03/F04を必ずBだけが検出するようには作らない。ただし、時刻情報だけで正のbyte読書きや主体を確定できない場合は、推測による違反認定ではなく保留とする。

**atime の扱いを事前登録する。**Linux 2.6.30 以降の既定である relatime では、前回の atime が現在の mtime または ctime 以前のとき、または前回の atime が1日より古いときだけ atime を更新する。strictatime は毎回更新する。[A2] そのため F03 を A1 が検出できるかは、mount option でほぼ決まる。

現行ホストでの観測(各1回。[A3]):

- ホストの ext4(relatime)では、作成直後の1回目の読取りで atime が更新され、1.1 秒後の2回目は更新されなかった。
- 非特権の user+mount 名前空間の中で tmpfs を `strictatime` で mount でき、2回とも更新された。同じ名前空間の既定 tmpfs(relatime)は1回目だけ更新された。
- ホストの ext4 を bind して strictatime へ remount することは拒否された(exit 32)。[R] 親から受け継いだ atime フラグが user 名前空間内でロックされるため(外れる条件: 別の理由で拒否された場合)。

これを踏まえ、次のとおり決める。

- protected-fixtures はケースごとに SUT sandbox の mount 名前空間の中で tmpfs として作る。ファイルシステムの種類を揃え、atime の方式だけを変えられるようにするためである。
- 主比較の protected-fixtures は `strictatime` で mount する。A1 を強い比較対象にするという 4.1 の方針に合わせる。
- 補助条件として、F03/F04 の A1 だけを `relatime` で追加実行する(12.2)。**fixture の作り方に注意が要る**(2026-10-19、[A11]): `utime` で時刻を設定すると ctime が現在時刻になるので、そのままでは relatime でも最初の読みで atime が更新される(atime ≤ ctime の規則)。「直前に読まれたファイル」を再現するには、時刻を設定した後に一度読んで、atime を ctime より後にする。実運用に近い条件で A1 の検出がどれだけ落ちるかを示す。
- relatime では「作成直後の最初の読取り」は atime を更新するため、fixture の配置から最初の request までの間に controller が読取りを行わない手順を固定する(行うと SUT の読取りが atime に現れなくなる)。
- どちらの条件でも、atime の変化だけで「正のbyte読取り」を確定しない。atime の更新は読取り試行の手がかりであり、byte 数と主体は確定しないため、A1 の判定は `INCONCLUSIVE` を上限とする。ただし契約が「保護対象の atime を変えない」を property として明示する場合はこの限りではない。

合成canaryの一致検査では、部分送信、分割送信、既知のencodingの扱いを事前登録する。暗号化・任意の変換・圧縮・側信道を含む完全な情報フロー検査とは呼ばない。

最終snapshotの一致は、一時書込みがなかった証拠にはしない。受信器が何も受け取らなかったことは、接続試行がなかった証拠にはしない(`docs/05` 5.5、`docs/06` 6.3 と同じ立場)。

### 9.2 Bの最小観測能力

最初のBはstraceを使い、固定したsyscall集合について引数・戻り値・PID/TID・時刻・FD情報を取得する。[S2][S3]

| 領域 | 必須の観測 | 不足時の扱い |
|---|---|---|
| ファイル取得 | open/openat/openat2の取得結果、FD、読取りmode | 未解決ならresource関連claimは未確定 |
| 内容のI/O | read/pread64/readv/preadv、write/pwrite64/writev/pwritev と正の戻り値 | byte数不明なら成功した読書きと断定しない |
| カーネル内転送 | sendfile/copy_file_range/splice の入出力FDと正の戻り値 | 入出力FDのどちらかが未解決ならresource対応を保留 |
| FD系譜 | close、dup系、fcntl(F_DUPFD系)、fork/clone/clone3、exec時の継承(O_CLOEXEC) | 対象FD不明ならresource対応を保留 |
| 通信 | socket、connectの宛先と戻り値、必要な送受信 | 試行・成立・送信・受信を区別 |
| プロセス | 起動時点、fork/clone/clone3/vfork、exec系の成功、終了 | 対象木の一部欠落は完全観測ではない |
| 書換え | rename/unlink/truncate等、fixtureが実際に利用する経路 | 観測未対応の変更経路を黙って許容しない |

カーネル内転送を必須に入れる理由: Rust の `std::io::copy` は、ファイルからファイルへのコピーで `copy_file_range` を使う。現行ホスト(rustc 1.97.1 と 1.99.1、release、1 MiB、各1回観測。[A3])では次のとおりだった。

| コピー | 観測した syscall |
|---|---|
| File→File、BufReader<File>→File | `copy_file_range` |
| File→TcpStream(127.0.0.1) | 8 KiB の `read` と `sendto` |
| File→stdout(pipe、通常ファイル)、File→FIFO、BufReader<File>→stdout | `read` と `write` |
| `fs::read` | `read` |

この範囲では `sendfile` と `splice` は呼ばれなかった(seccomp で呼出しを kill する設定でも確認)。ただし、std の特殊化の条件は読んでおらず、他の版・他の宛先型(ChildStdin、UnixStream 等)・入力サイズでは確かめていない [U]。C/C++ 実装は `sendfile` を直接使い得る。必須集合はこのまま残し、E0 で両実装の実際の syscall 列を取り、本試験前に確定する。scratch 領域から保護対象へのコピー(F04 の変形)では `copy_file_range` が使われ得るため、これを未対応にすると Rust 実装のファイル系シナリオが INCONCLUSIVE になる。

`-f` による子プロセスの追跡について、strace(1) の説明は fork/vfork/clone を挙げ、clone3 を明記していない。glibc の `posix_spawn` 等が clone3 を使う場合にも子を追えることは、E0 の校正項目「子プロセスとexec」で確かめる [R]。[S3]

**WP3 第1段の実装(2026-10-19、`normalize/`)**: 上の表の領域を strace の記録から読む。取り方は `strace -f -yy -q --decode-pids=pidns -s 0 -e trace=%file,%desc,%network,%process` に決めた。`--decode-pids=pidns`(strace 6.13)は、pid 名前空間の中の clone の戻り値に外側の pid を注記する。注記の無い記録では割り当て順で対応付ける [R]。mmap・io_uring・FD の受け渡し・rename 等は `unsupported_mechanism` として記録し、保護対象に関わる claim を INCONCLUSIVE にする。シナリオの実装が無いので、実装の syscall 列はまだ見ていない。

主要fixtureはblocking I/Oに限定する。EINPROGRESSを接続成功とみなさず、非同期接続の確認が必要なケースは別プロファイルにする。

mmap経由のファイル内容アクセス、io_uring、vmsplice、プロセス間FD転送、同時path置換等は、初回の内容アクセスclaimでは未対応とする。これらを使うことが判明したら、該当claimをINCONCLUSIVEとし、隠して通常のread/write監視の範囲に含めない。

### 9.3 path文字列とresource identity

`/allowed/../protected/x`という文字列だけで判定しない。requestの文字列、カーネルへ渡ったpath、解決先resource、FDからの読取りは分けて記録する。

初回は次の制約下でresource対応を成立させる。

1. fixtureのnamespaceを開始前に固定し、ファイル・symlink・hard linkの対応をcontrollerのcatalogへ登録する。
2. ケース中に対象のpath置換・mount変更・外部からのrenameを行わない。F04の内容書換えとpath置換は別扱いにする。
3. collectorのFD情報、open結果、固定catalogを突き合わせ、対応の根拠を保存する。
4. 同一性が一意に決まらないときは、文字列一致で穴埋めせずRESOURCE_UNRESOLVEDとする。

catalogはlogical ID、device/inode、namespace、初期digestを持つ。ただしdevice/inodeも再利用され得るため、ケースのlifetimeと固定namespaceの前提を含めて識別する。

この方式は、一般的なTOCTOUや同時path置換に耐える汎用的なresource同定を実現したという意味ではない。そこまで広げる場合はカーネル側の対象同定を追加し、別実験にする。

### 9.4 requestへの帰属

初回は一つのSUTプロセス群に一つのケースだけを与える。複数requestのシナリオも、その系列を唯一のケースとする。

principal、tenant、操作の許可条件はcontrollerのcase定義と承認済みpolicyから取得する。SUTがログへ出力した`authorized=true`を正解にしない。

起動段階、ready段階、request処理段階、終了段階を分離する。ただし、起動中に秘密情報を読み込んだものが後で使われ得るため、startupの作用を一律に捨てない。startupにも別の許可範囲を適用する。

並行request、非同期workerへの伝搬、複数tenantの共有process内でのイベント帰属は初回の範囲外とする。その能力を検証する際は、PIDや時刻だけでrequestを推定しない。

### 9.5 観測期間

開始は監視器の準備完了後、SUT起動前とする。終了は最後のrequestの応答時ではなく、以下をすべて満たした時点とする。

- 事前登録した待機期間を経過した。
- SUTとその子孫の終了を確認した。
- collectorのbufferを排出した。
- 正常に終了できなかった場合は、強制停止の時点と未確認範囲を記録した。

主比較の待機期間案は最後の応答後1秒、ケース全体の上限は60秒とする。これらは試験範囲であって、1秒以後に副作用がないことの保証ではない。

### 9.6 C0/Cの計装器とOS観測の組合せ

C0ではNulgrind、CではMemcheckを使用し、外側からBと同じ系統のOS収集を行う。straceとValgrindの組合せが対象環境で正常動作するかはE0の受入試験項目とする。未確認のまま本試験へ進めない。

Memcheckの機械可読レポートは対象の出力と分離し、エラー種別、PC／module offset、allocation/freeとの関係、対象digestに対応付ける。検査器による終了コード変更はSUTのクラッシュと区別する。

Valgrind の出力の分離と子プロセスの扱いは core manual に従って固定する。[S4b]

- テキスト出力は `--log-file`(または `--log-fd`。既定は fd 2 = SUT の stderr と同じ)で controller 領域へ出す。XML は `--xml=yes` と `--xml-file`(または `--xml-fd`)で別の出力先へ出す。これで SUT の stderr と混ざらない(E0 で、SUT の stderr は 0 byte のまま、診断は別ファイルに出た。2回。[A5])。ただし `--xml=yes` を付けた run では `--log-file` のテキストログが空だった(Valgrind 3.24.0、2回)。判定には XML を使い、テキストが要るなら別の run にする。
- `--trace-children` の既定は no で、fork した子は追うが、exec した先は計装しない。F06 のように補助実行ファイルを起動するケースでは、補助実行ファイルを計装するかどうかを事前登録する。主比較では SUT 本体だけを計装する案とし(`--trace-children=no`)、exec 先の作用は外側の OS 観測で見る。
- 外側の strace には Valgrind 自身の open(debuginfo、`.valgrindrc`)も SUT と同じ pid で出る(E0、2回)。B の集計は pid ではなく対象リソースで絞る。
- **最適化でメモリエラーが消えることがある。** E0 では `-O2` のビルドで、1 byte のヒープ越境が malloc・free ごと消え、Memcheck は何も出さなかった(解放後使用は `-O2` でも出た。2回)。C の正例は、評価する成果物そのもので witness を確かめる(7.2)。
- `--xml=yes` は Memcheck で `--leak-check=full` を伴う。leak は本実験の property に含めないので、leak の報告は判定に使わない。

runtime自身が行うメモリ確保、ライブラリ読込み、補助プロセス等を記録する。許可する例外は正確な対象と段階で登録し、Valgrind経由なら全部許可という例外を作らない。runtime作用とSUT作用が分離できない場合は、比較の限界として明示する。

Cの検出率は、対応するallocatorとエラー種別に限って解釈する。独自allocator、logical subregion、JIT、未対応命令などへ自動的に一般化しない。[S4][S6]

### 9.7 観測の完全性

完全性は「全OS動作を観測した」という意味ではなく、そのclaimが要求する、登録された経路の監視が継続したことを指す(`docs/06` 6.5 の gap の扱いと同じ)。

straceの未完了／再開行、途中終了、出力制限、解析不能、未知のsyscall、対象追加失敗を検出する。文字列の省略を対象の不在とみなさない。[S3]

eBPFへ置き換える拡張では、ring bufferの予約失敗や欠落を明示的に数える。bufferが満杯になる可能性があるため、イベントがないことだけで禁止動作不在を主張しない。[S9]

次を独立に記録する。

```text
monitoring_complete_for_scope  # 判定に必要な監視が継続したか
trace_storage_truncated       # 保存した詳細証跡に省略があるか
response_truncated            # 利用者への表示を省略したか
unsupported_mechanism_seen    # 未対応のアクセス経路が現れたか
```

表示を省略しても有効な反例は保持できる。一方、禁止イベントを探すための監視自体を失った場合、反例未発見を成功へ変換しない。

## 10. 判定モデルと証跡

### 10.1 4種類のevaluation

既存設計の区分を維持する(`docs/06` 6.4)。[I1]

| Evaluation | 意味 |
|---|---|
| VIOLATED | 現在の対象・契約・範囲に対する有効な反例がある |
| SATISFIED_IN_SCOPE | 必要な証拠が揃い、有限の登録範囲で条件を満たした |
| INCONCLUSIVE | 実行したが、欠測・対応付け不明・観測不足等で確定できない |
| NOT_EVALUATED | 未対応・未実行等により当該propertyを評価していない |

SATISFIED_IN_SCOPEは脆弱性不在の証明ではない。scopeは入力、状態、実行期間、対応I/O経路、メモリ検査の種類、環境を含む。

### 10.2 判定の優先順位

既存の採否(`docs/06` 6.6)をそのまま使う。

```text
subject_context が現在のものと一致しない                         → HOLD (STALE_CONTEXT)
現在のcontextで有効な必須claimの反例がある                       → REJECT
必須claimが NOT_EVALUATED / INCONCLUSIVE / 欠測 / 実行未完了     → HOLD
Policyが要求するplatform scopeを満たさない                       → HOLD (PLATFORM_SCOPE_UNMET)
有効なケースが0、または必要な状況に到達していない               → HOLD (VACUOUS_SCOPE)
それ以外                                                         → ACCEPT_WITHIN_SCOPE
```

本実験では、Policy の platform scope に「実行条件(A0/A1/B/C0/C)と環境版」を含める。B の結果で C を要求する Suite を ACCEPT にしない。

反例は欠測があっても、その反例に必要な証拠が健全なら成立する。一方、欠測のある実行で「反例がなかった」という主張は強められない。

特にA0が403を確認した場合、応答propertyは成立しても、読取り禁止propertyは未確認のままである。全体評価を安易にACCEPT_WITHIN_SCOPEへ上げない。

### 10.3 証拠イベントの例

以下は形式例であり、実測ログではない。実装時は自由文や仮のdigestを許さず、schemaとdigestの形式を検証する。

```json
{
  "schema": "mukoz-security-observation-event/1-draft",
  "run_id": "example-run-001",
  "case_id": "F03-case-09",
  "subject": {"role": "sut", "pid": 2001, "tid": 2001},
  "phase": "request",
  "event_kind": "file_read",
  "stage": "completed",
  "resource_id": "protected-fixture-1",
  "result": {"success": true, "bytes": 32},
  "attribution": {
    "case": "exclusive_process_group",
    "resource": "frozen_catalog_and_fd_lineage",
    "policy": "controller_approved"
  },
  "evidence_origin": "os_observer",
  "raw_evidence_ref": "trace/000042"
}
```

上のイベントは、該当resourceの読取りが禁止されるとき、SEC.FS.NO_FORBIDDEN_READの反例になる。秘密情報が外部へ開示された証拠には、それだけではならない。

### 10.4 runの保存内容

| 分類 | 必須項目 |
|---|---|
| 同一性 | artifact全体、依存群、contract、binding、fixture、input、policyのdigest |
| 実行条件 | mode、tool設定、compiler flag、OS/CPU、mount option、seed、環境制限 |
| 観測 | capability、開始／終了、欠落、未対応機構、帰属の確実性 |
| 判定 | property別 evaluation、method、execution、platform、independence、context_match(`docs/06` 6.4 の軸) |
| 反例 | 具体的なrequest系列、初期状態、failure predicate、必要な生証拠 |
| 比較 | root-cause group、実装系列、成果物種別、反復、実行順序 |
| 性能 | setup、startup、workload、drain、評価の各時間、CPU、RSS、ログ量 |
| 保留 | missing capability、ground-truth ambiguity、backend divergence |

`release_authorized` は false とする。OSのreadイベントが得られたことを、形式的なrefinement成立の証拠へ変換しない。

## 11. 固定入力比較の実行手順

### 11.1 E0: 校正

**実施状況(2026-10-19)**: 観測器そのものの校正(第1段)を専用 VM で行った。下の表のうち、FD・失敗 I/O・カーネル内転送・子プロセス・symlink・書込みと復元・ヒープ・論理領域・正常操作・正解ラベル用追跡器は、校正用プログラムで期待どおりだった(2回。[A5]、`e0/README.md`)。ログ欠落・parser 障害と実装の syscall 列は、parser(WP3)と実装(WP1)が無いので未実施。atime は strictatime と relatime の違いが出る条件で試していない。

まず実験の正例／負例より単純な、観測器校正用のプログラム群を用意する。E0 では公開ケースだけを使い、保留ケースは実行しない。

| 校正項目 | 確認すること |
|---|---|
| FDの再利用・dup | 別のファイルへのreadを誤帰属しない |
| 失敗するopen/read/connect | 試行と成功を混同しない |
| カーネル内転送 | sendfile/copy_file_range/splice の入出力resourceとbyte数を正しく対応付ける |
| 子プロセスとexec | 子孫の作用を失わず、collector自身をSUTと誤認しない |
| fixed symlink | 表示pathではなく登録された解決先へ対応付ける |
| 一時書込みと復元 | snapshot一致でも書込みイベントを検出する |
| API拒否後の副作用 | 応答後も登録期間内を観測する |
| atime | strictatime/relatime で A1 の manifest がどう変わるかを記録する |
| ヒープ越境・解放後使用 | Cで所定のメモリエラーを検出し、Rでは検出しない |
| 論理領域だけの違反 | Cが検出できない場合に安全と誤判定しない |
| ログ欠落・parser障害 | 成功へ変換せず、既存の反例は維持する |
| 正常な補助操作 | 許可範囲内の行動を過剰拒否しない |
| 正解ラベル用追跡器 | B の collector と独立に同じ作用を記録できる。両者の不一致は裁定対象として残す |
| 実装の syscall 列 | 両実装が 9.2 の必須集合の外の経路を使わないかを確認する |

Cが動作しない場合はA0/A1/Bの比較だけを完了させ、CはUNSUPPORTEDとして報告する。別の検査器へ無断で置き換え、同じC名で集計しない。

### 11.2 E1: 各条件を別々に実行

比較単位を以下とする。

```text
block = scenario × implementation × artifact_variant × case × repetition_seed
```

各blockの中でA0/A1/B/C0/Cをランダムな順序で実行する。順序は固定seedで作成し、条件の位置が全体として均衡するようにする。すべてAの後にBを行うような偏った順序は使わない。

1. artifact、契約、入力、binding、環境のdigestを検証する。
2. 前回のprocess、DB、volume、受信器ログを除去する。
3. ケース固有の合成resource、policy、seedを配置する。
4. 当該modeのcollectorと正解ラベル用追跡器を開始し、準備完了を確認する。
5. 固定argvでSUTを起動する。shell文字列として連結しない。
6. readyを確認し、固定されたrequest系列を送る。
7. 登録期間だけ観測し、終了・buffer排出・最終状態を確認する。
8. 生証拠と正規化イベントを別々に保存する。正解ラベル用追跡器の記録は検査側から読めない領域へ保存する。
9. evidence viewに対して同一の契約評価器を適用する。
10. 実行後に読み取り専用の正解ラベルと照合し、findingを裁定する。
11. 有効な反例にはreplayに必要な最小の証拠を関連付ける。

実行中に入力・契約・例外ルールを変更しない。環境障害があった場合は元の失敗runを残したうえで、事前登録した回数だけ再試行する。正常終了した回だけを残すことは禁止する。

### 11.3 E2: 同一traceのevidence projection

Bの一回の実行から以下を作って、同じ評価器で再評価する。

```text
πA0(trace_B) = API・終了に関する証拠だけ
πA1(trace_B) = API・外部状態の証拠だけ
πB(trace_B)  = A1 + OS証拠
```

同様にCの一回の実行から、メモリ診断を隠したviewと含むviewを作る。

```text
πB(trace_C) = C実行中に得た外部・OS証拠だけ
πC(trace_C) = 外部・OS証拠 + メモリ診断
```

E2は実行が同じなので、証拠を追加した効果を調べやすい。一方、そのtraceはB/Cの計測下で発生したものであり、計測なしのnative実行にも同じ動作があったとは保証しない。

したがって、E2はE1の代わりではなく、E1の検出差が証拠の増加によるものか、実行挙動の変化によるものかを説明する補助である。

### 11.4 挙動差の処理

正常入力でA1とB/Cの応答、外部状態、終了が一致しなければBACKEND_DIVERGENCEとして保存する。違反入力の場合も、違反が発生するかどうかがmodeで変わったケースは別集計にする。現行の differential test(`docs/06` 6.8)と同じく、不一致は片方を正とせず保存する。

Cだけでメモリエラーが確認できた場合は、次を分けて報告する。

- 計装実行で契約違反を検出したこと。
- native実行でも対応する欠陥・経路が存在すると確認できたこと。
- native実行で外部の被害やクラッシュを再現できたこと。

これらは同じ主張ではない。nativeでクラッシュしないからメモリ欠陥がない、とはしないが、計装器だけで生じた現象をnativeの確定的な被害としても扱わない。

## 12. 試験数・予算・段階化

### 12.1 Pilot

まずF01/F03/F05/F08/F10/F12の6シナリオ、各1実装で実施する。

```text
6シナリオ × 1実装 × 3成果物(V/R/H)
× 8ケース × 3反復 × 5条件 = 2,160 executions
```

**Pilotの8ケースは公開8ケース(正常2、許可境界2、禁止経路4)とする。保留8ケースは使わない。**目的は主にハーネス、ラベル、観測能力の成立確認である。Pilotの結果を使ったルール調整は許す。調整後の評価は本試験の保留ケースで行い、Pilot の結果を主要評価に混ぜない。

### 12.2 本試験

```text
12シナリオ × 2実装 × 3成果物(V/R/H)
× 16ケース × 3反復 × 5条件 = 17,280 executions
```

V/R/Hの成果物は合計72個。根本的なシナリオは12種類であり、17,280個の独立した脆弱性を検査したとは表現しない。二つの実装で24個のシナリオ実装blockを構成する。

17,280 executionsのうち、保留8ケースに対応する主要評価分は8,640 executions、公開8ケースの回帰・校正分は8,640 executionsとなる。

atime の補助条件(9.1)として、F03/F04 の A1 だけを `relatime` で追加実行する。

```text
2シナリオ × 2実装 × 3成果物 × 16ケース × 3反復 × 1条件(A1) = 576 executions
```

この576件は上の17,280件に含めず、補助結果として別表にする。

E2は取得済み証拠の再評価であり、原則としてSUT実行数を増やさない。E0、E3、反例再生、実行基盤の能力試験は上の件数に含まれない。

### 12.3 固定する資源予算案

| 項目 | 初期案 | 解釈 |
|---|---|---|
| 1 runのwall上限 | 全条件60秒 | 同一ケース比較用の十分な上限。セキュリティ契約の応答期限ではない |
| 最終応答後の観測 | 1秒＋終了／drain | その範囲以後の副作用不在は主張しない |
| 1 runのmemory上限 | 計装分を含むSUT群2 GiB | 超過はinfra／budgetと契約違反を切り分ける |
| 1 runの詳細trace | 16 MiBを上限候補 | 欠落するならclaimを保留。無断sampling禁止 |
| 同時実行 | 主比較は1 block内逐次 | host負荷の交絡を減らす |
| 本試験ログ予算 | 32 GiBを上限候補 | 超過前に停止・保存し、未実行数を報告 |
| infra再試行 | 最大1回 | 最初の失敗runも残し、availabilityへ反映 |
| 反復seed | 3個を事前固定 | canary、resource ID等を変える。独立の欠陥数とは数えない |

E0で予算が不適切と分かったら、本試験前に改訂する。本試験途中でCだけ上限を増やす場合は別の予算条件として扱い、主結果と混ぜない。

時間はsetup / startup / request_workload / drain / evaluation / totalに分解する。短いfixtureではstartupが支配的になり得るため、totalだけを「検査速度」と呼ばない。

## 13. 指標と統計解析

### 13.1 主指標: 追加検出

D_mを、mode mが主要評価用の保留ケースで有効な反例を示した根本原因IDの集合とする。入力の表記違い、反復、同じ不具合の複数イベントを別の欠陥として数えない。公開ケースでの検出集合は別に保持する。

```text
OS追加検出集合      = D_B \ D_A1
Memory追加検出集合  = D_C \ D_B
失われた検出集合    = D_A1 \ D_B、および D_B \ D_C
```

追加検出だけでなく、実行攪乱や未対応により失われた検出も必ず示す。modeを深くすれば常に集合が包含されるとは仮定しない。

また、次を区別する。

- witness単位検出: その具体的な入力・初期状態・propertyで反例を出せたか。
- root-cause単位検出: 同じ欠陥を少なくとも一つの登録ケースで検出できたか。
- 安定検出: 事前登録した対応ケースの3反復すべてで検出できたか。

F01〜F12を同じ重みで扱うmacro集計と、各カテゴリの個別値を出す。メモリ系のケースを増やすだけでCを有利にしない。

### 13.2 診断力と誤った成功を分ける

未確認は「欠陥を見つけた」とは数えないが、「誤って安全と断定した」とも同一視しない。

| 真の状態 | 評価結果 | 計数 |
|---|---|---|
| 対象違反あり | 正しくVIOLATED | 検出成功 |
| 対象違反あり | INCONCLUSIVE / NOT_EVALUATED | 診断上の未検出。保留として適切かも別記 |
| 対象違反あり | SATISFIED_IN_SCOPE | 誤った成立判定。ただし真の違反と同じscopeで比較する |
| 対象違反なし | 誤ってVIOLATED | false positive |
| 対象違反なし | SATISFIED_IN_SCOPE | 登録範囲の成立確認 |
| 対象違反なし | INCONCLUSIVE / NOT_EVALUATED | 保留。精度の成功にも失敗にも混ぜず割合を報告 |

必須propertyに違反があるのに全体をACCEPT_WITHIN_SCOPEへ上げた場合は、false admissionとして最優先で扱う。全条件がHOLDを返すだけでfalse admissionがゼロになっても有用とはいえないため、検出率・成立確認率・保留率を必ず併記する。

### 13.3 availabilityと条件付き性能

次の二つを併記する。

```text
実用検出率     = 検出できた登録正例 / 全登録正例
条件付き検出率 = 検出できた正例 / 当該modeで有効に実行・判定できた正例
```

実用検出率では未対応・失敗・時間切れも検出できなかったものとして分母に残す。条件付き検出率だけを示し、対応できない課題を隠してはならない。

同時に、未対応、観測欠落、infra障害、ground-truth未確定の件数を分ける。真の状態が確定していないケースは精度指標の正例／負例に入れず、件数と内容を報告する。

### 13.4 費用と再現性

| 指標 | 定義 |
|---|---|
| 実行時間倍率 | 対応block内のtotal_B / total_A1、total_C / total_B等 |
| workload時間倍率 | startup等を除いた対応区間の比率 |
| CPU・memory費用 | SUT群、collector、評価器、全体を分けて取得 |
| 追加欠陥あたり計算費用 | 追加core-seconds／追加根本原因数。追加0なら未定義と明記 |
| 証跡量 | raw bytes、正規化bytes、findingあたり必要なbytes |
| 導入工数 | observer共通実装と対象ごとのBinding作成を別々に記録 |
| 裁定工数 | findingの真偽確認に要した人の作業時間 |
| replay成功率 | 固定された対象・入力・環境で同じproperty違反を再現した割合 |
| false admission | 必須property違反があるのに採用可能とした割合 |
| 判断可能率 | 正しく違反／成立を確定できたproperty割合 |

入力縮小の実装は後から追加できる(現行 `mukoz shrink` は property と前提を保って縮小する。`docs/06` 6.10)。初回は、元のケースの正確なreplayを優先する。縮小時は同じfailure predicateを維持し、別のクラッシュへ変わった入力を「同じ反例の最小化」としない。

### 13.5 統計的扱い

本実験は、少数の設計されたシナリオに対する機序の検証であり、実際の開発案件からの無作為標本調査ではない。

- 主な効果量は、対応のある検出差、根本原因の追加件数、対応する費用比とする。主結果は保留ケースのみ、公開ケースは別集計にする。
- 不確実性はシナリオをclusterとしたbootstrapで95%区間を計算する。各cluster内の実装・ケース・反復・modeの対応を保つ。
- 12シナリオという少数clusterの区間は不安定になり得る。区間だけで一般的な優位を宣言せず、生の不一致表を提示する。
- 3反復や16ケースを独立サンプルとして扱って、人工的に小さいp値を作らない。
- メモリ、OS作用、業務意味のカテゴリ別結果を示し、総合順位だけにしない。
- timeoutまでに未検出だったrunは、time-to-first-findingの右打切りとして扱う。timeout値を「発見時間」として平均しない。

誤検出0件でも真の誤検出率0とはいえない。独立なn試行で0件の場合の片側95%上限は1 - 0.05^(1/n)となるが、R/Hやケースの相関がある本試験で、総実行数をそのままnに代入してはならない。

本試験は5ポイント等の小さな差を確実に検出できるような検出力を保証しない。一般化の判断が必要なら、反復を増やすのではなく、別の根本原因・実プロジェクトを増やす。

## 14. サンドボックスによる問題の隠蔽を調べる副実験

### 14.1 E3の対象

F03の禁止ファイル読取り、F05の禁止接続、F06の禁止execを対象とする。外側の安全境界は常に有効なまま、検証対象に関係する内側の強制制御だけを切り替える。

| 環境 | 設定 | 確認する対象 |
|---|---|---|
| APP_CHECK | 合成の禁止resourceへOS上は到達可能 | アプリ自身が契約を守るか |
| CONTAINMENT_CHECK | 同じresourceへの操作をOS等で拒否 | 配置構成全体が禁止作用を封じるか |

使用する強制制御はE0で対応範囲を確かめ、version・policy・適用結果を固定する。Landlockを採用する場合も、そのABIと対応するファイル／ネットワーク操作に限定し、すべての作用を制御できると仮定しない。[S12]

Landlock の範囲(公開文書の記述。[S12]):

- ファイルの実行(`LANDLOCK_ACCESS_FS_EXECUTE`)と読取り(`LANDLOCK_ACCESS_FS_READ_FILE`)は ABI 1(Linux 5.13)から。読取りの検査は open のときに行われ、read ごとではない。
- ネットワークは、リリース済みの範囲(man-pages 6.19 の表、ABI 9 / Linux 7.1 まで)では TCP の bind/connect だけで、ABI 4(Linux 6.7)から。開発版の kernel 文書(7.3-rc7)には ABI 10 で UDP が加わるとあるが、リリースされる版は確かめていない [U]。
- 現行ホストのカーネルは 6.12(man の表では ABI 6)である。F05 の禁止接続は TCP で作り、UDP を使うケースを CONTAINMENT_CHECK に入れない。

この区別は既存設計の「観測できるか」と「強制停止できるか」を別の値として持つ方針(`docs/06` 6.3)と同じである。

### 14.2 期待する結果の分離

以下は設計上の期待であり、実測結果ではない。

| 成果物 | 環境 | 起こり得る結果 | 正しい解釈 |
|---|---|---|---|
| V | APP_CHECK | 禁止読取り／接続／execが成立 | アプリ契約違反 |
| V | CONTAINMENT_CHECK | 試みたがOSが拒否 | 封じ込めは成立しても、アプリ側の問題解消とはしない |
| R | APP_CHECK | アプリが操作前に拒否 | 登録範囲でアプリの禁止条件を満たす |
| R | CONTAINMENT_CHECK | アプリが操作前に拒否 | OS拒否に依存しないケースの確認 |

実行が失敗した理由は、アプリの明示的拒否、OSの拒否、resource不存在、network不達、実行環境障害に分ける。HTTP 403だけで理由を判定しない。

### 14.3 最小規模

```text
3シナリオ × 1実装 × 2成果物(V/R)
× 2入力 × 3反復 × 2環境 × 2条件(A1/B)
= 144 executions
```

機能的な拒否だけを見る場合と、禁止試行を含めて見る場合の差を出す。試行禁止propertyと成功禁止propertyは別々に評価する。

## 15. 入力探索・診断ビルド・ARM64ルーチンの拡張実験

### 15.1 X1: 入力探索の寄与

E1で観測の寄与を確認してから、固定入力以外の探索を比較する。

| 探索条件 | 入力生成 | 判定に使う証拠 |
|---|---|---|
| G0 | 固定seedの非誘導変異 | 同じ固定済み評価器 |
| G1 | バイナリcoverageで誘導する変異 | G0と同じ固定済み評価器 |

AFL++にはbinary-only向けのQEMU／Frida等の経路がある。具体的なmodeは対象環境で能力試験を行って決め、速度に関する過去の資料の値をそのまま期待値にしない。[S10]

この比較では、coverage取得の有無以外の実行方式を可能な限り揃える。同じcoverage実行基盤で、G0はcoverageを取得しても次の入力選択には使わず、G1だけ選択へ反映する構成を優先する。

同じ初期corpus、辞書、mutation演算、seed数を使い、以下を別々に比較する。

- 同じwall time: 初期案は1 seedあたり15分、10 seeds。計装費用を含めた実用上の探索効率。
- 同じ実行数: 初期案は1 seedあたり10,000 executions、10 seeds。入力選択そのものの効率。

二つは別campaignとし、時間制限と実行数制限のどちらを先に満たしても同じ比較とすることはしない。

各探索器が発見した入力をunionして固定corpusを作り、A1/B/Cで交差再生する。この再生費用を含めたend-to-end時間も別途報告する。これにより、「よりよい入力を見つけた」のか「同じ入力で多くの違反を見た」のかを分離する。

最初はfile/CLIで入力できるnative処理を対象とし、HTTP全体の探索は別課題とする。診断用harnessで見つけた不具合は、可能な範囲で元のサービス入口にも再生し、対象の差を記録する。

### 15.2 X2: ASan診断ビルド

同一source revisionからrelease成果物とASan成果物を作る。ASan側はdiagnostic_artifact_digestとして別IDにし、releaseのdigestを共有しない。ASanはコンパイラ計装とruntime libraryを使うため、この分離が必要である。[S7]

比較対象はF10/F11と、Memcheckの対応外になりやすい追加のメモリケースとする。検出範囲、diagnostic message、runtime費用、source／buildへの依存を比較する。

ASan側だけで検出できた問題については、releaseのコード経路、allocation条件、最適化差を検討する。ASanでの違反を無条件にreleaseの実行証拠へ転用しない。

### 15.3 X3: 既存emulated executorでの途中アクセス観測

既存 Mukoz の emulated executor(raw routine)へ直接接続する、小さな別実験を置く。[I1] 既存設計はすでに、すべてのメモリアクセスの観測と強制、`allowed_read`/`allowed_write` の区別、「範囲外に書いて元へ戻す」違反を終了時比較では検出できないことを定めている(`docs/06` 6.2、6.3)。したがって X3 は新しい実行エンジンを作らず、既存 executor の観測を Binding で隠す／見せる比較として組む。

対象は、入力bufferから出力bufferへ有限長をコピーするルーチンとする。target platform は `x86_64/raw/sysv-x86_64/none` と `aarch64/raw/aapcs64/none` の両方(どちらも現行の engine qualification 済み。`docs/09` 9.8)とする。コードbytes、ABI、初期register、map済みmemory、許可read/write領域を固定する。

| 条件 | 観測 |
|---|---|
| R0 | 戻り値＋最終register／最終memory だけを Binding で評価する(途中アクセスの claim を Suite から外す) |
| R1 | R0＋`machine.memory.access` による途中のread/writeアドレス・サイズの評価 |

実行エンジンは両条件で同じ emulator program(既定の `mukoz-emu-icicle`。ADR-25)、同じコードbytes、同じ環境モデルにする。最初に共通traceを取得し、途中アクセスを隠すprojection比較を行う。その後、監視有効／無効の費用を別々に測る。Unicorn 版(`mukoz-emu`、ADR-24)は二つ目のエンジンとして差分確認に使ってよいが、必須ではない。[S13]

emulator は完全なOSや実行環境を提供するものとして扱わない。

用意する成果物は、正常版と次の変異版である。

- 許可範囲外へ一時的に書き、元の値へ戻す。
- map済みで読取り可能だが、契約では禁止された領域を読む。
- 出力結果は一致するが、scratch以外の領域へアクセスする。
- 正当なscratch stack利用。これは違反としない(既存 fixture の scratch stack / red zone と同じ扱い。`docs/09` 9.7)。

重要なのは、禁止領域もモデル上はアクセス可能にして、監視によって契約違反を検出することである。すべて未mapにすると、R0でもクラッシュで見つかり、途中アクセス観測の寄与を分離できない。

途中アクセスだけが違う変異では、R0の契約が実際に観測する最終register・memory・戻り値を正常版と一致させる。戻り値だけ一致して最終registerの差で発見できるケースを、途中観測の独占的効果としない。

この実験はF12に対応する論理領域Bindingの価値を小さく検証できる。ただし証拠の範囲は emulated routine であり、native プロセス、OSのファイルアクセス、実機のメモリ安全性とは別である。

### 15.4 X4: 実在プロジェクトでの確認

主比較で機序が確認できた後、公開された修正前後の成果物を3〜5件の初期集合として追加する。LSCBenchのケースを採用する場合も、license、依存取得、再現条件、安全性を個別確認する。[S1]

人工的なfixtureと実在コードの結果は別集計にする。コンテナがbuildできない、OS観測が対応しない、正解が不明なケースを隠さず、選定条件と除外理由を全件記録する。

ここでも最初から450課題や複数LLMの生成を再実行しない。観測器の寄与を問う実験に、モデル差・仕様生成差・探索差を同時に持ち込まない。

### 15.5 X5: AArch64 Linux での追試

主比較の後、Pilot と同じ6シナリオを `linux-aarch64` の環境版で再実行する。目的は、strace・Valgrind・正解ラベル用追跡器が AArch64 Linux で同じ capability を持つかの確認と、結果のホスト依存の把握である。x86-64 の結果を AArch64 へ転用せず、AArch64 の結果は別の環境版として報告する。macOS・Windows ではOS観測器が異なるため、本実験の範囲外とする。

## 16. Mukozへの実装構成

### 16.1 実験ハーネスの構成案

以下は新規コードの配置案である。判定(evaluation と採否)は新規に作らず、既存 Mukoz の claim 軸と採否を再利用する方針とする。

既存の判定部への接続点(`src/` を読んで確認。付録A1):

- property ごとの判定 `judge::judge_case`(`src/judge.rs:80-279`)は Executor の結果型 `emu::Observation` に結合しており、ensures から syscall やイベントを参照する手段は無い。ここは使わない。
- 集約 `judge::aggregate`(`src/judge.rs:299-345`)と採否 `judge::admit`(`src/judge.rs:357-392`)は、ケースごとの `CaseClaim` だけを受け取る。外部の evidence はここへ入れられる。
- 最小の切り出し [R]: 正規化イベント列を `Vec<CaseClaim>` に変える関数を1つ書き、`check()` の property 一覧(`src/run.rs:418-425`)と各ケースの claims(`src/run.rs:469-499`)に合流させる。`aggregate` と `admit` は変えない。外れる条件: 既存 property(`effects.no_forbidden` 等)の値を上書きする場合は、`src/run.rs:267-277` と同じ「後から書き換える」形が要る。
- 保存される run には集約後の claims しか残らない(`src/run.rs:693-731`)。保存済みの run に外部イベントを後から足して判定し直すことはできないので、E2 の projection は本ハーネス側で正規化イベントを保存して行う。
- **WP0.5a の実装(2026-10-19、branch `wp05-external-claims`、commit 0fe2323・417fe1f・0eeb529、未 merge)**: `mukoz check … --external-claims <file>`(schema `mukoz.external_claims/0`、実験的)。file は subject context と case ごとの claim を持つ。Mukoz が受け付けるのは、新しい `external.*` property と、`NOT_EVALUATED`/`REQUIRED_CAPABILITY_UNAVAILABLE` の claim の置き換えだけで、Mukoz 自身の VIOLATED は消せない。context の不一致・未知の case・重複・形の誤りは入力エラー(exit 2)になる。未知の eval は `NOT_EVALUATED` になり、独立性を宣言しない file は ACCEPT を HOLD に下げる。run には file の sha256 と、file が宣言した収集器が残る。`aggregate`・`admit`(`src/judge.rs`)は変えていない。イベントから claim への変換はハーネス側に置く(WP0.5b、`adapter/`。下)。**既知の制限**: case は ID で結ぶので、regression store が変わると、context が同じでも case の中身がずれ得る。実験中は suite と store を固定する(`docs/07` に記載)。
- **WP0.5b の実装(2026-10-19、`adapter/`)**: 正規化イベント(`secobs.observations/0`)と binding(`secobs.binding/0`)から `mukoz.external_claims/0` を作る。証拠にするのは OS 観測器と Memcheck の、SUT に帰属が確実なイベントだけで、SUT の出力は使わない。健全な反例は欠測があっても VIOLATED、欠測・未解決・binding の失効・空の範囲は INCONCLUSIVE、capability が無ければ NOT_EVALUATED。既存 property を external 結果の論理積で埋める `fills` を持つが、意味のずれがあり本試験で使うかは未決定 [R]。実際の strace・Memcheck の記録からイベントを作る部分は WP3・WP4。
- Executor には trait が無い(`emu::Executor` は具体型、`nproc::run` は自由関数)。B/C を Mukoz の Executor として組み込むかは WP3 で決め、初回はハーネス側で実行して claim だけを合流させる。

```text
research/security-observation/
  DESIGN.md                      # 本設計書
  experiment.lock.yaml           # 本試験の固定条件・予算・seed・mount option
  corpus/
    contracts/                   # 承認された契約
    policies/                    # principal/resource/operationの許可表
    cases/                       # 固定入力と初期状態（public/ と holdout/ を分ける）
    bindings/                    # artifactごとの対応表
    artifacts/                   # digestで管理する実行成果物
  calibration/                   # 観測器・判定器の健全性試験
  controller/                    # run順序・隔離・lifecycle管理
  collectors/
    api/
    external-state/
    strace/
    memcheck/
  normalize/                     # event正規化とresource/subject対応
  adapter/                       # 正規化イベントを既存 Mukoz の claim / evidence へ写す
  replay/                        # 反例再生
  analysis/                      # 集計・対応差・費用・不確実性
  reports/                       # 生成されるMarkdown/JSON/CSV
```

正解ラベル、修正差分、正解ラベル用追跡器の記録は、検査側から読めるこの木の外に置く。比較用opaque IDから正解側IDへの対応表は裁定担当だけが持つ。`corpus/cases/holdout/` は本試験開始まで検査側の権限で読めないようにする。

### 16.2 中核へ戻すデータ型

```text
ObservationCapability
  kind / version / platform / supported_scope / limitations

EvidenceRequirement
  property_id / required_event_semantics / attribution / completeness

ObservationEvent
  subject / phase / operation / resource / stage / result / origin

ScopeReceipt
  input_domain / time_window / mechanisms / gaps / instrumentation

Counterexample
  artifact / contract / environment / case / failure_predicate / evidence
```

property ごとの評価は新しい型を作らず、既存の claim 軸(evaluation、method、execution、platform、independence、context_match)を使う。

単に`supports_security=true`とは表現せず、`successful_read_count`、`resolved_resource_identity`、`heap_invalid_access`など、判定に必要な能力の単位で示す。

**設計と実装のずれ:** `docs/02` 2.5 は「Planner が claim ごとに必要な capability を計算して照合する」と書くが、その汎用の照合は実装されていない(付録A1)。現行の実装は、host probe(`src/host.rs:444-478`)、隔離 capability の固定一覧と policy による許可(`src/policy.rs:61-69`、`src/policy.rs:144-183`)、claim の手書き書き換え(`src/run.rs:252-277`)に分かれている。観測 capability を入れる型も無い。新しい能力名を足す場合は、少なくとも probe の追加と、`src/run.rs:267-277` の書き換えを「capability が確認され、イベントがあれば判定、無ければ NOT_EVALUATED」へ変える必要がある [R]。汎用の照合を実装するか、この手書きの形を続けるかは、本実験の外の設計判断として扱う。

### 16.3 責務を分ける

| 要素 | 責務 | 行わないこと |
|---|---|---|
| Contract | 許可・禁止・不変条件 | SUTのログから正当性を後付けする |
| Planner | 必須証拠に対応するExecutor・capabilityと予算を選ぶ | 取得できないpropertyを勝手に削除する |
| Collector | 生の実行事実と取得範囲を返す | アプリの許可policyを独自に決める |
| Binding | resource、principal、memory領域等を対応付ける | 曖昧な対応を確実と偽る |
| Evaluator(既存) | 型付き条件を証拠へ適用する | LLMの確信度を合否にする |
| Assessor(既存) | 反例・欠測・scopeから採否を返す | 本番の実行権限を発行する |
| Supervisor | 安全な実行・終了・資源制御 | OSが阻止しただけでアプリを合格にする |

### 16.4 実装範囲を抑える

初回は、既存のstraceとMemcheckを再利用し、Mukoz側では正規化、契約評価への接続、証跡、scope、replayに集中する。

一般的なバイナリ逆解析、全ISAのメモリ計装、汎用eBPFフレームワーク、任意アプリの認可状態推定を同時に実装しない。

CLI は現行のコマンド(`docs/07` 7.2)に対応付ける。

| 本実験の段階 | 現行コマンド |
|---|---|
| 成果物の登録・形式確認 | `mukoz inspect <file>` |
| 計画の固定 | `mukoz plan <suite.toml> --artifact <file>` |
| 実行 | `mukoz run <plan-id>` |
| 採否の再発行 | `mukoz assess <run-id>` |
| 証跡の部分取得 | `mukoz show <id>` |
| 反例再生・縮小 | `mukoz replay <counterexample-id>` / `mukoz shrink <counterexample-id>` |
| capability の確認 | `mukoz platform probe` / `mukoz platform show` |

strace・Valgrind を使う実行が `native-process` の capability 追加になるのか、別 Executor になるのかは WP3 で決める。新しい Executor 名や引数が実装済みという前提でコマンドを提示しない。

**WP3 での決定(2026-10-19)**: SEC-OBS-01 の間は、B(と C)を Mukoz の Executor にも capability にもしない。ハーネスが SUT sandbox の外で strace を動かし、正規化イベント → claim に変えて、`check --external-claims`(WP0.5a)で合流させる。理由: 合流の口だけで比較実験ができる。Executor にするには、16.2 の capability 照合と probe の設計が先に要る。どちらにするかは、本実験の結果を見て WP8 で決める。

## 17. 作業パッケージと完了条件

| ID | 作業 | 主な成果物 | 完了条件 |
|---|---|---|---|
| WP0 | 実験仕様と環境を固定 | experiment lock、scope、隔離検査、ツール導入の判断 | 専用 VM(2026-10-19 決定)を用意し、strace・valgrind・audit/bpftrace の導入版と guest kernel を固定する。予算・版・seed・mount option・権限が固定され、通常環境へfallbackしない |
| WP0.5a | Mukoz 側の合流 | `check --external-claims`、`mukoz.external_claims/0` | **2026-10-19 実装済み・未 merge**(16.1)。`aggregate`・`admit` を変えずに、人工の claim から REJECT/HOLD/ACCEPT が出る。build・`cargo test`・`tier1.py` は exit 0。astra のレビュー2巡で残った指摘は 0 |
| WP0.5b | ハーネス側の変換 | 正規化イベント → `mukoz.external_claims/0` の変換、7.5 の負の試験 | 人工イベントで 7.5 の負の試験が通る(read>0→-1、sendfile>0→0 で違反が消える、欠測で成功を返さない、SUT の偽 PASS を昇格させない、空の範囲を成功にしない)。**2026-10-19 に `adapter/` で満たした(試験 40 本、故障注入 20 通りをすべて検出、実際の `mukoz check --gate` で 15 場面が期待どおり。各1回、[A7])。Mukoz 側の `VACUOUS_SCOPE` は試していない** |
| WP0.7 | SUT sandbox を作る | 名前空間・試験用ネットワーク・tmpfs の fixture・cgroup | 現行 `native-process` の隔離は流用しない(8.2)。隔離に失敗したら起動しない。外側の strace・Valgrind で SUT を追跡できる。**2026-10-19 に受入試験 41 項目を2回とも満たした(`sbx/`、[A6])。常駐する SUT の起動・要求・停止、wall time と出力量の上限は未実施** |
| WP1 | Pilotの契約とfixtureを作る | V/R/H、正常系、正解witness | 6シナリオで正常系成立、違反／許可のラベルを説明できる。**2026-10-19 に `fx/`(付録 A9)。6シナリオ × V/R/H × 公開8ケースの 144 run で、R/H は全ケース、V は正常系・許可境界で FUNC が通った。SUT の外の観測による違反は、事前ラベルと 144 run すべてで一致した(各1回)。保留8ケース・2系統目の実装・3反復 × 5条件は未実施** |
| WP2 | A0/A1を実装 | API・state・sink観測 | APIと外部状態の正例／負例を過不足なく判定。**2026-10-19 に `fx/a1.py`(付録 A11)。tracer なしの 144 run と relatime の 24 run で、同じ run の正解に対し誤った違反・誤った成功は0件。A0 は開示と応答、A1 はそれに加えて接続の成立と配送を正解どおりに判定した。atime だけの変化は INCONCLUSIVE に留めた** |
| WP3 | Bを実装・校正 | strace parser、FD系譜、scope receipt、Executor/capability の位置づけ | F03等の作用、失敗I/O、カーネル内転送、子孫、欠測を区別できる。**2026-10-19 に第1段(`normalize/`、[A8])。校正用プログラムの trace 60 本で、失敗した I/O・カーネル内転送・子孫・symlink・一時書込み・強制停止を期待どおりに区別した。故障注入 20 通りもすべて検出した。位置づけは「ハーネス側 + `--external-claims`」に決めた(16.4)。WP1 の公開ケースで strace を付けた 96 run(F01/F03/F05/F08)を通し、判定はすべてラベルどおりで INCONCLUSIVE は無かった(付録 A9)。保留ケース・2系統目の実装・request への帰属は未実施** |
| WP4 | C0/Cを実装・校正 | Nulgrind/Memcheck adapter | 同一artifactで動作し、診断出力を分離、F12の限界を保留できる。**2026-10-19 に `normalize/memcheck.py` と `fx/`(付録 A10)。同じ sha256 の 18 artifact が C0・C の下で 288 run とも FUNC とファイル・通信の正解を保ち、診断は fd 4 に分かれた。Memcheck は F10 V の3件だけを検出し、F12 の領域 property は NOT_EVALUATED のままだった(各1回)。費用の記録は未実施** |
| WP5 | E1/E2/E3を実行 | run一式、projection、反例 | 実行数の一致、同一性、失敗run保存、再生が確認できる。**2026-10-19 に E1 の形だけを公開ケースで実行した(付録 A12、4,320 run、1回)。計画した 4,320 run がすべて揃い、条件間の挙動の差・誤った VIOLATED・誤った SATISFIED は0件。保留ケース・別の作者の実装・E2・E3・反例の再生は未実施** |
| WP6 | 本試験へ拡張 | 12シナリオ×2実装、保留ケース | 評価器を固定し、適用範囲外を明示して実行できる |
| WP7 | 集計・設計判断 | Markdown report、machine-readable結果 | 追加検出・失った検出・誤検知・保留・費用を報告 |
| WP8 | 有効な機能だけ中核へ戻す | capability／evidence requirement設計 | 本実験の `linux-x86_64` の結果と、他ホスト(macOS、Linux arm64、Windows)での collector の可用性を混ぜず、ホストごとに統合判断できる |

WP1〜WP4は薄い縦切りで交互に進めてよい。Cの導入が難航しても、A1/B比較の結果を出せる構造にする。

## 18. 採用・保留・中止の判断基準

### 18.1 正しさの必須条件

以下は検査機能を採用する最低条件とする。

- 校正試験で、成功／失敗、主体、対象、scopeの意味を取り違えない。
- 現在の必須契約に反例があるのに、ACCEPT_WITHIN_SCOPEを返さない。
- 観測不能・未対応・欠測・stale Bindingを成功へ変換しない。
- 修正済み版とhard negativeの違反判定に、説明できない誤検知が残らない。
- 追加検出について、対象・入力・環境を固定したreplayが可能である。

0件の誤りを観測しても一般的な無誤りを保証しないが、既知の誤判定を残したまま標準機能にしない。

### 18.2 有用性の仮置き基準

以下は外部研究で確立された閾値ではなく、Mukozの投資判断を曖昧にしないための本実験向けの事前基準案である。本試験前に固定する。

| 判定 | 基準案 |
|---|---|
| Bを追加 capability へ進める | A1に対する追加検出が異なる2シナリオ以上であり、各々2実装・対応ケース3反復で再現。追加作用を正しい契約へ対応付けられる |
| Bを日常実行の候補にする | 上記に加え、対象scopeで実行・判定可能率95%以上、block対応のtotal時間比B/A1の中央値が3倍以内 |
| Bを診断専用にする | 追加検出はあるが、費用・権限・対応範囲により日常実行基準を満たさない |
| Cを任意メモリ検査として採用 | Bに対する追加検出がF10とF11の両方で再現し、計装依存とnative側の欠陥を区別して説明できる |
| Cを限定的な定期試験候補にする | 上記に加え、対応対象で実行・判定可能率95%以上、total時間比C/Bの中央値が10倍以内。対象限定で判定する |
| Cを標準必須にしない | メモリ系にしか追加価値がない、または対応／費用が限定的。これは失敗ではなく選択的backendという結論 |

C の採用基準の厳しさについて: C の追加検出候補は F10 と F11 だけであり(F12 は検出できないことを確かめる限界対照)、基準は実質的に「両方で再現」を要求する。これは意図した厳しさである。片方だけで再現した場合は「不採用」ではなく「保留」とし、メモリ系シナリオを追加した次の実験版で判断する。

判定可能率95%等の分母となる「対応scope」は、本試験前のcapability宣言から固定する。失敗結果を見て対応対象を減らさない。全シナリオに対する実用検出率・未対応率も必ず併記する。

時間比だけで最終判断せず、絶対時間、startup費用、追加欠陥あたり費用、Binding工数も併記する。straceが遅いことをもって、OS観測という考え自体が無価値とは結論しない。追加検出が成立した後に、eBPF等の費用改善を別途試験する。

### 18.3 中止・方向変更条件

- Bの追加検出がA1の観測を不当に弱くしたためにしか生じない場合、B優位の主張を撤回する。atime を relatime にしたときだけ B が優位になる、などがこれに当たる。
- Cの追加検出がruntime固有の問題だけで、対象成果物へ結び付かない場合、対応する結論を保留する。
- file/resource/requestの対応付けにケース固有の手作業が大量に必要なら、汎用エンジンではなく限定対象の検査として再評価する。
- 観測の取得費用より、正しい契約・正解ラベルを作る費用が支配的なら、その事実を設計判断へ反映する。
- 既存検査器の結果を取り込むだけで十分なら、独自の低レベル検査器を作らない。

**Memcheckがメモリエラーを見つけたという結果だけでは、Mukozの独自性は実証されない。**Mukoz固有の価値は、正しい契約への対応付け、未確認の管理、再現証跡、既存の意味モデルとの接続に残るかを評価する。

## 19. 妥当性の限界と解釈禁止事項

| 限界 | 対応・解釈 |
|---|---|
| 人工fixtureが低レベル側に有利 | A1、業務意味系、hard negative、F12を含め、X4を別実施 |
| 正解が自作実装の理解に依存 | witnessと別系統診断、ラベル裁定履歴を残す |
| 正解ラベル用追跡器と B が同じ誤りを持つ | 有無は VFS 層(fanotify)で、byte 数も VFS 層(bpftrace の VFS 関数 kprobe)で取る。どちらも B と同じ syscall から呼ばれる経路を見るので、独立は部分的。不一致を裁定対象として残す |
| 観測自体が実行を変える | E1/E2/C0を分け、divergenceを報告 |
| nativeと計装のメモリ配置差 | 計装下の反例とnativeの被害再現を区別 |
| 途中のファイル操作がmetadataに現れる | A1で正当に検出できたものはA1の実績にする。mount option ごとに別表 |
| 不在を断定する証拠が不足 | scopeとcapabilityを明示し、未確認を保留 |
| 少数・非無作為のシナリオ | 一般的な脆弱性率やモデル能力を推定しない |
| 再ビルド・別ISA・別OS | 同一成果物の実験と分離し、結果を転用しない。AArch64 は X5 で別に扱う |
| 高い権限を要する観測器 | 実環境への導入可能性を別の採用条件にする |
| 計装器やcollectorの不具合 | 校正・version固定・生証跡保存・別系統の照合 |
| 同一process内の診断器 | 敵対的SUTに対する完全な改竄耐性は主張しない |

禁止する結論は次のとおり。

- 「Cが最も深いので最も安全」
- 「403が返ったので禁止アクセスはなかった」
- 「openが成功したので秘密情報が漏れた」
- 「OSが止めたのでアプリの脆弱性は修正されている」
- 「Memcheckエラー0なので任意のメモリアクセス制約を満たす」
- 「ASanビルドが成功したのでreleaseも検証済み」
- 「17,280回実行したので17,280個の独立した証拠がある」
- 「テスト成功なのでFSLからbinaryへのrefinementが証明された」
- 「linux-x86_64 で B が有効だったので macOS でも有効」

## 20. 結果レポートの形式

### 20.1 冒頭に置く判断

```text
OS観測: 採用候補 / 限定採用 / 保留 / 不採用
メモリ計装: 任意診断 / 定期試験候補 / 保留 / 不採用
確認できた追加価値:
確認できなかった価値:
適用可能なホスト・成果物・契約:
未対応・観測不能・未実施:
```

### 20.2 主結果表

実験前に数値を埋めない。未実施を0で表現しない。

| 指標 | A0 | A1 | B | C0 | C |
|---|---|---|---|---|---|
| 実行済み／計画件数 | 未実施 | 未実施 | 未実施 | 未実施 | 未実施 |
| 有効な反例を得たroot-cause数 | 未実施 | 未実施 | 未実施 | 未実施 | 未実施 |
| 主要比較相手への追加検出数 | 対象外 | 未実施 | 未実施 | 対象外 | 未実施 |
| 主要比較相手から失った検出数 | 対象外 | 未実施 | 未実施 | 対象外 | 未実施 |
| false positive | 未実施 | 未実施 | 未実施 | 未実施 | 未実施 |
| false admission | 未実施 | 未実施 | 未実施 | 未実施 | 未実施 |
| 保留／未評価property数 | 未実施 | 未実施 | 未実施 | 未実施 | 未実施 |
| 正常ケースでのbackend divergence | 未実施 | 基準 | 未実施 | 未実施 | 未実施 |
| total時間中央値・p95 | 未実施 | 未実施 | 未実施 | 未実施 | 未実施 |
| workload時間中央値・p95 | 未実施 | 未実施 | 未実施 | 未実施 | 未実施 |
| peak memory・ログ量 | 未実施 | 未実施 | 未実施 | 未実施 | 未実施 |

A1 の relatime 補助条件(12.2)は別表にする。

### 20.3 個別findingに必要な説明

```text
Root cause:
Contract property:
対象artifact / 環境:
入力 / 初期状態:
A1では何が見えたか:
B/Cでは何が追加で見えたか:
違反と判断する最小の証拠:
試行 / 成功 / 情報開示のどれを確認したか:
native / instrumented / emulatedのどの範囲か:
正解ラベルの根拠（使った追跡器を含む）:
反例replay結果:
修正済み版・hard negativeでの結果:
追加費用:
限界・未確認:
```

### 20.4 実験から戻す設計判断

最後に、contractごとの最小必要観測表を作る。

| 契約 | A1で足りるか | Bが必要か | Cが必要か | 別の意味Bindingが必要か | 推奨する実行条件 |
|---|---|---|---|---|---|
| 指定チャネルへの秘密出力禁止 | 未評価 | 未評価 | 未評価 | 未評価 | 未決定 |
| 保護ファイルの読取り禁止 | 未評価 | 未評価 | 未評価 | 未評価 | 未決定 |
| 不許可exec禁止 | 未評価 | 未評価 | 未評価 | 未評価 | 未決定 |
| allocator範囲のメモリ制約 | 未評価 | 未評価 | 未評価 | 未評価 | 未決定 |
| tenant認可 | 未評価 | 未評価 | 未評価 | 未評価 | 未決定 |
| 論理メモリ領域の読取り禁止 | 未評価 | 未評価 | 未評価 | 未評価 | 未決定 |

この表を元に、利用者が単にLevel 1/2/3を選ぶ方式ではなく、契約に必要な証拠からExecutor・capabilityを選び、足りない能力を `REQUIRED_CAPABILITY_UNAVAILABLE` として明示する現行方式の拡張が有用かを判断する。

## 21. 実験開始前チェックリスト

- [ ] 本試験用の契約、許可表、入力、seed、label、tool version、mount optionを固定した。
- [ ] API応答とOS操作、試行と成功、内容取得と開示を分離した。
- [ ] A1に合理的な外部状態観測を与えた(protected-fixtures は strictatime)。
- [ ] V/R/Hが同じ正常系と同じセキュリティ契約で比較できる。
- [ ] Vの違反に到達するwitnessと、R/Hの根拠を用意した。
- [ ] 正解ラベル用追跡器が二層(VFS 層の有無、ptrace と別機構の byte 数)で、B の collector と同じ仕組みを使っていない。
- [ ] strace・valgrind・追跡器が導入され、版を記録した。
- [ ] evaluatorが正解ラベルや修正差分を読めない。
- [ ] 保留ケースを E0・Pilot で一度も実行していない。
- [ ] A0/A1へMemcheck等の診断チャネルが漏れない。
- [ ] collector、harness、runtime、SUTの作用を区別する。
- [ ] FDの系譜、カーネル内転送、resource対応、観測期間、未対応経路を規定した。
- [ ] 無観測・欠測・時間切れを成功へ変換しない。
- [ ] VMの外部経路とhost共有を遮断し、合成データだけを使う。
- [ ] アプリ検査と封じ込め検査を別環境として記録する。
- [ ] 全条件で対象artifactのbytesと依存群を固定した。
- [ ] C0/Cの実行差、ASanの別成果物、emulated routineの別scope、AArch64 追試の別環境版を区別する。
- [ ] 実行順序・再試行・中止・ログ予算を固定した。
- [ ] 追加検出だけでなく、失った検出と保留も報告する。
- [ ] 繰り返し実行を独立した脆弱性の数として扱わない。
- [ ] 結果を見てから採用基準や例外ルールを書き換えない。

## 22. 参考資料

以下の公開資料は、draft.3 の事前調査(2026-10-19)で実在と依拠する記述を確かめた(付録A2)。確認は取得したページを要約して行い、全文は通読していない。長いページの末尾は読んでいないものがある。公開ドキュメントの記載はツールの存在・性質の根拠であり、本実験環境での動作・性能・統合完了の証明ではない。実装時は採用versionと設定を固定する。

### 内部資料

[I1] Mukoz `docs/`(Mukoz 0.1.0、設計 revision 0.5)。本書が参照した章:

- `docs/01-concept.md`(保証の境界、P5)
- `docs/02-platform-model.md`(独立軸、ホスト別の実行可否、capability、Tier)
- `docs/05-execution.md`(native-process、effect model)
- `docs/06-evidence-and-judgement.md`(メモリ条件、観測と強制、claim 軸、gap、採否、differential test、反例)
- `docs/07-interface.md`(コマンド、`mukoz/1`)
- `docs/09-implementation-plan.md`(段階計画、fixture、Tier 1 受入基準)
- `docs/10-decisions.md`(ADR-24/25: emulator program の分離と既定)
- `docs/12-process-and-modules.md`(プロセス境界の effect model、native-process の隔離と観測範囲)

draft.1 が参照していた `Mukoz_Binary_Test_Tool_Design.md`(0.4-design-draft.1)は廃止され、上記 `docs/` に置き換わっている。本書は既存ファイルを変更しない独立した実験設計書である。

### 公開資料

- [S1] Tianyi Wu ほか(15名), Last Secure Code Benchmark: Is the Code You Vibe Secure?, arXiv:2610.14766v1, 2026-10-10 — 機能試験とセキュリティ試験を分けて評価する("graded by separate functional and security test suites")。150 の実在脆弱性シナリオを3粒度に分けた 450 課題。観測階層の比較自体は本書独自の実験提案。著者・日付は arXiv を正本とする(HF の紹介ページは著者が14名で、日付は 2026-10-14)。<https://arxiv.org/abs/2610.14766> / <https://arxiv.org/html/2610.14766v1> / 紹介ページ: <https://huggingface.co/papers/2610.14766>
- [S2] strace — official website。Linuxのprocess/kernel間の相互作用を観測し、対象の再コンパイルを必要としないツール。<https://strace.io/>
- [S3] strace(1) — upstream manual, man7.org掲載(確認時は strace 7.1)。syscallの引数・戻り値、子プロセス追跡(`-f` の説明は fork/vfork/clone を挙げ、clone3 を明記しない)、FD表示(`-y`/`-yy`)、文字列省略(`-s`、ファイル名は省略しない)、未完了／再開イベント等。<https://man7.org/linux/man-pages/man1/strace.1.html>
- [S4] Valgrind — Memcheck manual。メモリエラー検出とその意味、対応するallocationや診断の範囲。<https://valgrind.org/docs/manual/mc-manual.html>
- [S4b] Valgrind — core manual。`--log-fd`/`--log-file`、`--xml`/`--xml-fd`/`--xml-file`、`--trace-children`(既定 no)、`--child-silent-after-fork`。出力の分離と子プロセスの扱いの根拠。<https://valgrind.org/docs/manual/manual-core.html>
- [S5] Valgrind — Nulgrind manual。追加の検査を行わないValgrind tool。実行基盤の対照として参照。<https://valgrind.org/docs/manual/nl-manual.html>
- [S6] Valgrind — Supported Platforms。AMD64/Linux、ARM64/Linux等の対応対象。個別環境での能力試験は別途必要。<https://valgrind.org/info/platforms.html>
- [S7] LLVM/Clang — AddressSanitizer。コンパイラ計装とruntime libraryを利用するメモリ診断。<https://clang.llvm.org/docs/AddressSanitizer.html>
- [S8] Tetragon — Events。processおよびkernel tracing eventの収集。将来のOS observer候補として参照。<https://tetragon.io/docs/concepts/events/>
- [S9] Linux Kernel — BPF ring buffer。buffer予約・消費の仕組みと予約失敗等の扱い。<https://docs.kernel.org/bpf/ringbuf.html>
- [S10] AFL++ — Fuzzing binary-only programs。QEMU、Frida、Unicorn等を利用するbinary-only探索。性能記載を本実験の予測値には使用しない。<https://aflplus.plus/docs/binaryonly_fuzzing/>
- [S11] Frida — Stalker。動的な制御フロー追跡とarchitecture依存性。coverageとセキュリティ判定を区別する根拠。<https://frida.re/docs/stalker/>
- [S12] Linux Kernel — Landlock: unprivileged access control(確認時は 7.3-rc7 の文書)と landlock(7)(man-pages 6.19)。アクセス制御とABI／対応範囲。観測と強制制御を分離する際の参照。<https://docs.kernel.org/userspace-api/landlock.html> / <https://man7.org/linux/man-pages/man7/landlock.7.html>
- [S13] Unicorn Engine — official website。X3 の二つ目のエンジン候補(現行 Mukoz の既定は icicle-emu)。<https://www.unicorn-engine.org/>

### 事前調査で追加した資料

- [A2-1] mount(8) — relatime / strictatime / noatime の atime 更新規則。<https://man7.org/linux/man-pages/man8/mount.8.html>
- [A2-2] fanotify(7)、fanotify_mark(2) — FAN_ACCESS の発火条件、byte 数を持たないこと、mmap 経由を報告しないこと、FAN_Q_OVERFLOW、FAN_PRE_ACCESS(Linux 6.14〜)。<https://man7.org/linux/man-pages/man7/fanotify.7.html> / <https://man7.org/linux/man-pages/man2/fanotify_mark.2.html>
- [A2-3] auditctl(8)、audit.rules(7) — syscall ルール、`-F exit=`、syscall ルールの性能上の注意。<https://man7.org/linux/man-pages/man8/auditctl.8.html> / <https://www.man7.org/linux/man-pages/man7/audit.rules.7.html>
- [A2-4] bpftrace — `tracepoint:syscalls:sys_exit_read` の例。<https://raw.githubusercontent.com/bpftrace/bpftrace/master/docs/tutorial_one_liners.md>

**本実験の最終的な問い:** 低レベル化によってログを増やせるかではなく、必要な契約を、より正確に、再現可能な証拠と許容できる費用で判断できるようになるか。

## 付録A. 事前調査の記録(2026-10-19)

draft.2 に残した [U]/[R] を、三つの調査で確かめた。本文の [A1]〜[A12] はこの付録を指す。各観測は1回ずつで、一般化しない。

### A1. リポジトリ(commit f018494、読み取りのみ)

| 問い | 結論 | 根拠 |
|---|---|---|
| 判定部に外部 evidence を入れられるか | 集約と採否は `CaseClaim` だけを受け取るので入れられる。property ごとの判定は `emu::Observation` に結合しており使えない | `src/judge.rs:80-279`、`src/judge.rs:299-345`、`src/judge.rs:357-392`、`src/run.rs:418-425`、`src/run.rs:469-499` |
| native-process の隔離を流用できるか | そのままは不可(8.2 の5点) | `src/image.rs:248-252`、`src/host.rs:109-111`、`src/host.rs:208-233`、`src/host.rs:31`、`src/nproc.rs:208-237` |
| capability 照合はどこか | 汎用の照合は未実装。probe、policy、claim の手書き書き換えに分かれる | `src/host.rs:444-478`、`src/policy.rs:61-69`、`src/policy.rs:144-183`、`src/run.rs:252-277` |
| `effects.no_forbidden` を NOT_EVALUATED にする箇所 | native-process 単独のとき、SATISFIED だけを書き換える。信号で止まった場合の VIOLATED は残る | `src/run.rs:267-277`、`src/judge.rs:168-172` |

読まなかった範囲: `src/native.rs` の本体、`src/emu.rs` の大部分、`src/plan.rs`(grep のみ)、`src/expr.rs`、`src/store.rs`、`src/show.rs`、テスト。

### A2. 公開資料

| 資料 | 実在 | 確かめた記述 | 確かめられなかった記述 |
|---|---|---|---|
| S1 | Y | 機能とセキュリティの試験の分離、450 課題、著者・日付 | HTML 本文の末尾約3万字 |
| S2/S3 | Y | 再コンパイル不要、`-f`、`-y`/`-yy`、`-s`、unfinished/resumed | `-f` が clone3 を追うことの明記 |
| S4/S4b | Y | XML・ログの出し先・`--trace-children` は core manual にある | Memcheck 章の末尾約7千字 |
| S5/S6 | Y | Nulgrind は計装なし。AMD64/Linux・ARM64/Linux 対応、ARM64/Darwin なし | — |
| S7 | Y | コンパイラ計装と runtime、検出対象 | CPU アーキテクチャ別の対応表 |
| S8 | Y | イベント種別、JSON/gRPC 出力 | イベント損失の扱い("Event throttling" のページは開いていない) |
| S9 | Y | 空きが無ければ予約は失敗し、ブロックしない | カーネルがドロップ件数を数えるか |
| S10/S11/S13 | Y | binary-only の各モード、Stalker の方式、Unicorn の対応 ISA | S10 のモード別 ARM64 対応、S11 の末尾約1.8万字 |
| S12 | Y | 14.1 の ABI 範囲 | ABI 10/11 のリリース版 |

### A3. ホストでの実測(Linux 6.12.113+deb13-cloud-amd64、Debian 13、非特権)

| 項目 | 結果 |
|---|---|
| ツール | 有: gcc 14.2.0(libasan)、rustc 1.97.1/1.99.1、unshare、systemd-run、fanotify ヘッダ。無: strace、valgrind、clang、bpftrace、auditctl、perf |
| Rust `io::copy` | 9.2 の表のとおり。strace の代わりに自作の ptrace トレーサで記録し、seccomp の kill フィルタで突き合わせた。トレーサ自身は C で copy_file_range/sendfile/splice/fork+exec を呼ぶ陽性対照で確かめた |
| C プログラムの観測 | ptrace で openat・read の byte 数・clone・子の execve が見えた。ASan は heap-buffer-overflow を検出し、レポートを SUT の stderr と別のファイルへ出した(最初のエラーで停止) |
| atime | 9.1 のとおり |
| fanotify(非特権) | `FAN_CLASS_NOTIF` は EPERM。`FAN_REPORT_FID` 付きの init と inode への mark は成功し、open/read/close が1イベントで届いた。`FAN_MARK_FILESYSTEM` は EPERM |
| 名前空間・cgroup | 8.2 のとおり |

実測で使ったプログラムとトレースは、作業セッションの scratchpad(`/tmp/claude-1000/…/scratchpad/hostcheck/`)に置いた。一時領域なので、再現には E0 で作り直す。

見ていないもの: Valgrind に関わること全部、std のソース、`systemd-run --user` による cgroup の作成、外側の追跡器が名前空間を越えて追えるか、各観測の2回目以降。

### A4. 専用 VM の照合(2026-10-19)

管理エージェントの作成報告を受け取り、作業用ホスト(192.168.68.76)から `ssh obs@192.168.68.91` で次を1回ずつ確かめた。ssh のホスト鍵は初回接続で受け入れた(ECDSA SHA256:68EOgL71E+2WfyYBlO+g0hkv2ufsEHn+PLv8tu0UOUI)。報告と照合で食い違ったものは無い。

| 項目 | 自分で確かめた結果 |
|---|---|
| 構成 | hostname `sec-obs-01`、8 CPU、メモリ合計 7 GiB 表示(8 GiB 割当) |
| 版 | 8.1 に書いたとおり(`uname -r`、`dpkg-query -W`、`rustc +<版> --version`、`gcc --version`)。auditd は active |
| 外向きの遮断 | `curl https://deb.debian.org` は名前解決で、`curl https://1.1.1.1` は接続でタイムアウト(どちらも exit 28)。作業用ホストとゲートウェイ(192.168.68.1)への ping は失敗(exit 1)。作業用ホストの 22/tcp への接続はタイムアウト(exit 124) |
| VFS kprobe | 7.2 に挙げた関数すべてが `bpftrace -l` に出た。1 MiB の `/dev/shm` ファイルで、`cat` の `vfs_read` 合計 1,054,084 byte(8回)、`cp` の `vfs_copy_file_range` 合計 1,048,576 byte、その内側の `do_splice_direct` 1回(7.2) |
| 共有 | virtiofs・9p のマウントは無し(`/proc/mounts` で0件)。`/dev/virtio-ports` は無し。`~/.ssh` には `authorized_keys` だけ |

報告で受け取っただけで、自分では確かめていないもの: Proxmox 側の規則(`/etc/pve/firewall/103.fw` の中身、DHCP・NDP・RA の遮断、MAC フィルタ)、IPv6 での遮断、VM 内で root になっても規則を外せないこと、スナップショット `sec-obs-01-base` の存在、イメージのチェックサム検証。

誰も確かめていないもの: 作業用ホスト以外から 103 の 22/tcp が閉じていること(管理エージェントの実行が許可されなかった。規則の上では DROP)。ARP は IP 層の規則の対象外で通る(報告)。

運用上の注意(報告): 隔離のためにデータセンター全体のファイアウォールを有効にした(既定は ACCEPT、ホスト自身は対象外)。ホストの空きメモリは約 3 GiB。

### A5. E0 第1段(観測器の校正、2026-10-19)

専用 VM で、校正用プログラム `e0/cal.c` を strace・Nulgrind・Memcheck の下と直接実行で動かし、正解ラベルの二層(fanotify・VFS 関数の bpftrace)と突き合わせた。手順・ファイル・項目ごとの結果は `e0/README.md` にある。

- 判定: `e0/check.py`。修正後の2回(`e0/runs/r2`・`r3`)とも exit 0。49 run のうち 47 が期待どおりで、残る2つは期待値そのものを調べる run だった(pid 名前空間の中の遅い子、`-O2` のヒープ越境)。期待値を5か所壊した自己試験(`--mutate`)では、壊した 11 run をすべて検出した。
- 1回目(`r1`)には手順の誤りが2つあった(`-O2` で heap の欠陥が消えた、controller が fixture を読んで atime を動かした)。直してから r2・r3 を取った。r1 は照合に使っていない。
- 見ていないもの: ログ欠落・parser 障害、実装の syscall 列、atime の strictatime と relatime の違い、fanotify のキューあふれ、通信系・プロセス系の正解ラベル、Valgrind の `--trace-children`、費用の測定。

### A6. WP0.7 SUT sandbox の受入試験(2026-10-19)

専用 VM で `sbx/test.sh` を流し、`sbx/check_sbx.py` で 41 項目を判定した。修正後の2回(`sbx/runs/s5`・`s6`)とも exit 0。作りと項目は `sbx/README.md` にある。

- 主な結果: SUT は非 root・capability なし・no_new_privs で、pid 1 ではない。controller 領域・ssh 鍵・外側の fixture は読めない。外への経路は無いが、試験用ネットワークの受信器2つには接続できた。書けるのは case ごとの tmpfs と fixture だけだった。検査を壊すと exit 3 になり、SUT は起動しなかった。E0 の校正項目9つは、sandbox を通しても strace と正解ラベルの二層で一致した。Memcheck は sandbox の中で動いた。pids の上限が効いた。
- 試験の途中で見つけて直したもの: SUT が `/dev/mqueue` と `/run` の下(ホストと共有)に書けた。SUT の cwd が、隠したはずの controller ディレクトリを指したままだった。判定器の scope の誤りが2件あった。
- 見ていないもの: 常駐する SUT の起動・要求・停止、strictatime の mount option の検査、wall time と出力量の上限、カーネル・名前空間の突破への耐性。

### A7. WP0.5b ハーネス側の変換(2026-10-19)

作業用ホストで `adapter/` の3つの試験を1回ずつ流した。規則と場面ごとの結果は `adapter/README.md` にある。

- `test_adapter.py`: 40 本すべて通った。7.5 の各項目(read の戻り値、sendfile の戻り値、principal の変更、観測の開始・途中・終了の欠落、対象・帰属の未解決、binding の失効、反例の後の欠測、SUT の偽の PASS と偽の JSON、空の範囲)と、入力の形の誤り(exit 2、何も書かない)を見る。
- `mutate.py`: adapter を 20 通り壊し、どれも1本以上の試験を落とした(exit 0)。
- `e2e.py`: Mukoz `0eeb529` の release build(`cargo build --release --locked` が再ビルドなしで終わることを確かめた)と native-process の hello suite で、15 場面すべてが期待した採否と exit code になった。REJECT・HOLD・ACCEPT_WITHIN_SCOPE と `STALE_CONTEXT` が、変換器を通して出た。
- 見ていないもの: 実際の観測からのイベント生成(WP3・WP4)、Mukoz 側の `VACUOUS_SCOPE`、イベントの phase と観測期間の照合、request への帰属、別系列のレビュー。

### A8. WP3 第1段 strace の正規化(2026-10-19)

作業用ホストで `normalize/` の試験を流した。専用 VM では、sandbox を通した校正 9 項目と grace 0 を、`--decode-pids=pidns` 付きで取り直した(`sbx/runs/p1`)。規則と結果の詳細は `normalize/README.md` にある。

- `e0_check.py`: 記録済みの E0 r2・r3(30 run)、WP0.7 s5・s6(20 run)と、新しい p1(10 run)の計 60 run で、二つの表がすべて一致した。(1) 正規化イベントから集計したファイルごとの byte 数・open・失敗した試行が、正解ラベルの二層と一致した期待値に等しい。(2) adapter に通した判定が、項目ごとの期待値に等しい。grace 0 の run では、どの property も SATISFIED にならなかった。p1 は `e0/check.py` の三層照合でも 9 項目とも一致した。
- p1 の 10 本から pid 名前空間の注記を取り除いて、割り当て順で対応付けた。結果は、注記による対応 394 件のすべてと一致した(1回の取得)。
- `test_strace.py` は 46 本すべて通った。`mutate.py` は 20 通りの故障注入をすべて検出した。adapter を変えた後に、WP0.5b の試験(45 本、故障注入 23 通り、`e2e.py` の 15 場面)も再実行し、すべて通った。
- 作る途中で、実際の trace に当てて parser の誤りを5つ見つけ、直した(`normalize/README.md`)。
- 見ていないもの: シナリオ F01〜F12 と実装(WP1)、実装の syscall 列、request への帰属と段階、thread、inode による同定、別系列のレビュー。

### A9. WP1 Pilot の fixture と正解(2026-10-19)

専用 VM で `fx/run.sh` を1回流した(`fx/runs/w1`、144 run)。判定は作業用ホストで行った。詳細は `fx/README.md` にある。

- 6シナリオ(F01/F03/F05/F08 は Rust、F10/F12 は C)× V/R/H × 公開8ケース。各シナリオの禁止経路4ケースのうち1つは、V でも違反に届かない入力にした。
- `witness.py`: 144 run すべてで、SUT の外の観測による違反が、実行前に登録したラベルと一致した。R/H は全ケース、V は正常系・許可境界で FUNC が通った。観測に使ったのは fanotify・VFS bpftrace・試験用ネットワークの SYN・受信器の accept・成果物への gdb の access watchpoint で、SUT の出力と strace は使っていない。
- `witness.py --mutate`: 証拠の写しを 11 通り壊し、すべて検出した。初回は1通りを見逃した(fanotify が open と access を1行にまとめていて、注入が効いていなかった)。
- `b_check.py`: strace を付けた 96 run を `normalize/` → `adapter/` に通した。B の判定は、すべてラベルどおりだった(VIOLATED は V の禁止経路だけで、INCONCLUSIVE は0件)。
- 常駐する SUT を止めるため、sbxinit の `main_start` と `sbx.sh pid`・`term` を足した。変更後に WP0.7 の受入試験を再実行し、41 項目を満たした(`sbx/runs/s7`)。
- 見ていないもの: 保留8ケース(未作成)、2系統目の実装、Pilot の 3反復 × 5条件、strace なしでの同じ作用、A1、Memcheck、別系列のレビュー。

### A10. WP4 Memcheck の正規化と C0・C の実行(2026-10-19)

専用 VM で `fx/run.sh` を `FX_MODE=c`(Memcheck)と `FX_MODE=c0`(Nulgrind)で1回ずつ流した(`fx/runs/c-1`、`fx/runs/c0-1`。各 144 run、strace つき)。照合は作業用ホストで行った。

- 18 artifact の sha256 は、native(A9)・C0・C で同じだった。288 run のすべてで、FUNC と、SUT の外の観測によるファイル・通信の違反が、native のラベルどおりだった。SUT の stdout・stderr はすべて 0 byte で、Valgrind の出力は fd 4 だけに出た。
- `c_check.py`: C0・C とも 144/144 が期待どおりだった。ファイル・通信の判定は B と同じ。`mem.heap.addressable` は、C で F10 V の禁止経路3件だけが VIOLATED で、C0 では全件 NOT_EVALUATED。Memcheck の報告した行(`f10.c:32`)は、native の gdb の watchpoint と同じだった。F12 の `mem.region.read_authorized` は C0・C とも 24 件すべて NOT_EVALUATED。
- `normalize/test_memcheck.py`: 14 本すべて通り、故障注入 9 通りもすべて検出した。adapter は論理領域の述語を足した後、試験 47 本・故障注入 25 通りがすべて通った。
- 見ていないもの: Valgrind の下での時間と memory の費用、`-O0` のビルド、実際に途中で切れた XML、exec 先の計装、別系列のレビュー。

### A11. WP2 A0・A1 の判定(2026-10-19)

専用 VM で `fx/run.sh` を `FX_MODE=a`(tracer なし)で1回流し(`fx/runs/a-1`、144 run)、relatime の補助条件(F03、`fx/runs/a-rel`、24 run)を取った。照合は作業用ホストで行った。

- `a1.py`: 144/144 と 24/24 が clean。誤った VIOLATED・誤った SATISFIED は A0・A1 とも0件。F03 の V の読みは、A0 では NOT_EVALUATED、A1 では strictatime で INCONCLUSIVE(atime の変化)、relatime(直前に読まれた fixture)では NOT_EVALUATED だった。F05 の拒否された接続の試行は、A1 では見えなかった。
- relatime の最初の取得では、`utime` が ctime を進めたために atime が更新され、補助条件になっていなかった。fixture の作り方を直して取り直した(9.1)。
- `test_a1.py` は 16 本すべて通り、故障注入 10 通りもすべて検出した。
- Pilot の公開ケースでの条件ごとの検出(予備的、`fx/README.md`): F03 は B・C0・C だけ、F05 の拒否された試行は B・C0・C だけ、F10 は C だけが VIOLATED にした。F12 はどの条件も VIOLATED にしなかった。R・H への誤った違反は、どの条件でも0件。主比較ではない(公開ケース、1実装、各1回、作者1人)。
- 見ていないもの: 保留ケース、2系統目の実装、反復、A1 の DB 状態(Pilot に DB を使うシナリオが無い)、書込み(F04)の manifest 判定、別系列のレビュー。

### A12. E1 の形を公開ケースで実行(2026-10-19)

保留ケースを作らないまま、E1 の駆動部と集計を公開ケースで回した(代表判断「2」)。計画は `fx/e1plan.py`(864 block、条件の並びは 120 通りの順列を均等に回す、block の順は固定 seed で混ぜる)、反復は `fx/seed.py`(canary の末尾4文字を seed ごとに変える)。専用 VM で4 worker に分けて並列に流した(block の中は逐次)。判定器の hash は結果を見る前に `fx/runs/e1-pub.FREEZE.txt` に記録し、集計時に照合して一致した。

- 2系統目の実装 `fx/src2/` は C で、`openat` + ディレクトリ fd、`pread`、`sendfile`、`writev`、非ブロッキング `connect`、構造の違う heap record(F10、F12)を使う。作者は1系統目と同じ(私)で、変えたのは仕組みだけ。作者の独立性は満たしていない。
- 試し走り(60 run)で、2系統目の F05 V に B の誤った SATISFIED が出た(`NO_FORBIDDEN_CONNECT`)。`EINPROGRESS` の後を追わないのに、禁止先へ接続していないとしていた。WP3 の試験がこの誤りを期待値として固定していた。adapter を INCONCLUSIVE `OUTCOME_UNOBSERVED` に直し、試験と故障注入を足した(公開ケースでの調整、12.1)。
- 本走 `fx/runs/e1-pub`: 4,320/4,320 run。誤った VIOLATED・誤った SATISFIED・A1 と低層の矛盾・条件間の挙動の差・欠けた条件は0件。違反でない property の INCONCLUSIVE は B と C0 に1件ずつで、どちらも並列に動く他 worker の netns を `ip` が開いた行(`(deleted)` 付き)を parser が読めず、観測を不完全としたもの [R: 並列実行の副作用。逐次なら出ない見込み、確かめていない]。
- 根本原因(シナリオ × 実装、12個)の検出: A0 4、A1 6、B 8、C0 8、C 10。B \ A1 = F03(両実装)、C \ B = F10(両実装)、A1 \ B と B \ C は空。どれも3 seed すべてで検出(安定)。F12 はどの条件も検出しない(限界対照として想定どおり)。
- witness 単位(違反のある 74 の case × property): A0 42、A1 46、B 62、C0 62、C 68。B が A1 に足したのは F03 の open/read 12 と F05 の拒否された試行 4。
- 時間(1 run の中央値、4並列の負荷下): A0 2.43 s、A1 2.39 s、B 3.49 s、C0 3.77 s、C 4.04 s。block 対応の比の中央値は B/A1 1.43、C/B 1.18。fixture が小さく起動が支配的なので、検査の速さとは呼ばない(12.3)。
- 18.2 の仮基準に当てると、B の「A1 に対する追加検出が異なる2シナリオ以上」は根本原因単位では F03 の1シナリオで満たさない(F05 は A1 が p05 で根本原因を捉えるため、B の追加は witness 単位だけ)。C は F11 が無いので判断できない(保留)。ただしこの判定は公開ケース・同じ作者の実装によるもので、基準の適用ではない。
- 見ていないもの: 保留ケース、別の作者の実装、12シナリオへの拡張、E2 の projection、E3、反例の再生、逐次実行での時間、別系列のレビュー。

### A13. F11をVMなしで再現・検出（2026-10-20）

`fx/src/f11.c` にV/R/Hと、同じ初期化・ハンドラをソケットなしで実行する `F11_STDIO` を追加した。公開8ケースの事前ラベルは `fx/contract/F11.json`、独立したhardware watchpointは `fx/gdb/F11.gdb`、実行・照合は `fx/f11_local.py` にある。F11は1実装のみなので、E1の計画器は契約の `implementations: [1]` に従う。

`fx/runs/f11-local-2/` で、V/R/H×8ケース×native/GDB/Nulgrind/Memcheck×3反復の288実行を行い、照合エラーは0件だった。GDBとMemcheckは、V/p05・p06・p08だけを3反復とも検出した。native・C0・Cの216応答はすべて契約に合い、対象の違反ケースでも200応答を保った。Cの72判定はVIOLATED 9件・SATISFIED_IN_SCOPE 63件。正規化器とadapterを経て `mukoz.external_claims/0` を保存し、メモリcapabilityのないnative・C0はNOT_EVALUATEDを保った。同じバイナリを全条件で使用し、ソース・評価コード・成果物のhashも記録した。独立レビューは未実施で、independenceはselfである。

解放後使用のXMLにより、正規化器が解放stackを確保stackとしていた不具合が分かった。`free_frame` と `alloc_frame` を分け、実記録を使った負の試験9件と既存の正規化器の14試験・故障注入9通りを通した。最初の探索実行はwatchpoint再表示の数え方で照合エラーを出したため、修正後に全件を実行し直した。

これはstdio経路での校正であり、既存のHTTP/OS観測実験との統合比較や保留入力での主比較ではない。18.2のCの採用判断は保留を維持する。詳細・再実行手順は `fx/F11.md`。VMで開始した試験は接続断で記録一式を回収できず、結果には含めない。
