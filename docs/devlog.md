# 開発記録

1行ずつ追記する。観測は回数を書く。

| 日付 | 工程 | 記録 |
|---|---|---|
| 2026-10-04 | 1-A | `unicorn-engine` 2.1.5 は bindgen が libclang を要求しビルド不能(このホストに libclang なし)。2.1.1(同梱C、bindgen不要)は `arch_x86`+`arch_arm`+`arch_aarch64` でビルド成功(cargo build 約10秒、1回) |
| 2026-10-04 | 1-A | x86_64: `lea rax,[rdi+rsi];ret` で max+2=1、`sub` で 5-7=0xff..fe。sentinel 到達で停止、RSP=入口+8。mem hook が push/pop/ret の読み書きを幅付きで報告。未割当書込み=`WRITE_UNMAPPED`、`ud2`=`INSN_INVALID`、`syscall` は insn hook で捕捉(各1回) |
| 2026-10-04 | 1-A | aarch64: `8b010000; d65f03c0` で max+2=1(0.4由来のbytesがaddとして動くことを1回確認)。**同一インスタンスでコードを書き換えて再実行すると、書換え前の命令が実行された(5+7=12)**。翻訳キャッシュの持越し。→ ケースごとに新しいインスタンスを作る |
| 2026-10-04 | 1-A | 速度: 新インスタンス+add64 1件 を 4,160回で約0.66秒(フック有無で差は見えず、1回計測) |
| 2026-10-04 | 2・3 | 単一crate `mukoz` で Core(式・契約・計画・判定・Store)と emulated executor を実装。docs 9.2 の複数crate構成と worker プロセス分離は未実施(探索中の逸脱。限界として出力の `limitations` に `engine_runs_in_process` を出す)。Suite の `contract`/`binding` は ID ではなくパス |
| 2026-10-04 | 3 | Unicorn は不正命令でも code hook を呼び、命令長に `0xf1f1f1f1` を渡す。範囲外移動と誤判定していた → 不正命令として扱う(ud2 fixture で発見) |
| 2026-10-04 | 3 | 受入試験 4本(fixture 11個): 正しい4種 ACCEPT、変異5種が予告どおりの性質で REJECT、無限ループ・ud2 が HOLD、fail-fast。単体12本。すべて通過(1回)。回帰ケースは次回の先頭で再発を検出(1回) |
| 2026-10-04 | 3b | 領域・ポインタ・状態の3課題(copy、strlen、checked_inc)と AArch64 add64 を追加。正しい実装 ACCEPT、変異4種が予告どおり REJECT(1回) |
| 2026-10-04 | 3b | 64bit未満の引数はレジスタ上位ビットに乱数を入れるよう変更(SysVで不定)。`mov rax,rdi` 変異で検出、乱数化を外す故障注入で試験が落ちることを確認(1回) |
| 2026-10-04 | E1 | 式言語に `count` を追加、`ite` を遅延評価に変更(選ばれない側の0除算で INCONCLUSIVE になっていた)。`cargo test --bin` だけでは `target/debug/mukoz` が更新されず、古いバイナリで判定していたことに気付いた |
| 2026-10-04 | E1 | 比較試験の8課題(abs_diff, smax, popcount, sat_add_u32, fill, count_byte, reverse, checked_mul)。Mukoz: 参照実装8 ACCEPT、変異8 REJECT。reverse の最初の変異は等価変異だったので差し替え(1回) |
| 2026-10-04 | E1 | 隠し判定(C ランナー: fork + seccomp strict + guard page + canary + callee-saved 番兵、Python 参照実装): 参照実装8 PASS、変異8 FAIL(各1,500ケース、1回) |
| 2026-10-04 | 4 | 比較試験 第1群(abs_diff, fill, popcount, checked_mul × 条件A/B、各1回): 8件とも1回目で完成、隠し判定 PASS、Mukoz判定も ACCEPT。難易度が低く条件差は出ない |
| 2026-10-04 | E1 | 生成器に `max`(上限式)と依存順の並べ替えを追加。難課題4つ(memmove 重なりあり、hex_encode、shl_var、isqrt)を追加: Mukoz で参照 ACCEPT・変異 REJECT、隠し判定で参照 PASS・変異 FAIL(各1回)。memmove の戻り値はアドレスで契約(ISA非依存)に書けないため要件から外した |
| 2026-10-04 | 4 | 比較試験 x86 計22件(生成16: 8課題×A/B、修正6: count_byte・memmove・isqrt × A/B): 全件1回目で隠し判定 PASS・Mukoz ACCEPT。Mukoz ACCEPT かつ隠し判定 FAIL は0件。条件差は出ない([R] 課題が易しすぎ、修正課題の不具合も読めば分かる規模) |
| 2026-10-04 | E1 | 条件差の出る状況として異ISA(x86ホストで aarch64 を生成)を追加。独立判定器 eval/oracle/a64.py(Unicorn非依存の A64 整数部分集合インタプリタ、範囲外は unsupported=FAIL)。手書き符号化の参照/変異 4組: Mukoz と a64.py が8件全一致(各1回)。a64.py 自体の故障注入(callee-saved破壊・範囲外読み・SIMD・無限ループ)4件すべて FAIL |
| 2026-10-04 | 4 | 比較試験 計30件(生成 x86 16・aarch64 6、修正 x86 6・aarch64 2): 全件 PASS、誤った合格0、REJECT 0。結果と解釈を docs/11 §11.2 に記載。反例→修正ループは未観測 |
| 2026-10-04 | 4 | SWAR 課題(count_byte_fast、x86・aarch64 × A/B)4件: 全件 PASS。全員が厳密な零バイト判定を選び、罠(借り伝播の誤検出)は踏まなかった。Mukoz・隠し判定とも罠の変異は検出(各1回) |
| 2026-10-04 | E1 | 実在形に近い修正課題: gcc -O2 -fPIC の base64(384B、表込み)に1バイトの変異。初回 -fno-pic で作った版は絶対番地参照で、Mukoz・隠し判定とも memory 違反/クラッシュで一致して落とした |
| 2026-10-04 | 4 | gcc 修正課題 A/B: 両方 PASS。RA は手で逆アセンブルして特定し Mukoz は確認1回のみ。計36件・誤った合格0・REJECT 0 |
| 2026-10-04 | 4→R | 契約作成の試験(エージェントが spec と docs だけで契約・Binding・Suite を書く)hex_encode: 作成された契約は私の参照を ACCEPT・変異を REJECT(1回)。報告された問題: (1) requires が 528 中 525 を捨てても ACCEPT(3件) (2) bytes 変数の `max`/`bytes` 不正値が黙って無視 (3) `len` > max_len が黙って切り詰め (4) docs と実装の食い違い多数 |
| 2026-10-04 | R | 再設計: (1) 有効ケースが下限 min(100, 生成数/4) 未満なら HOLD `LOW_ADMITTED_CASES`、`[limits] min_admitted_cases` で明示上書き、捨てた件数を limitations に常に出す (2)(3) 型に合わない生成器項目・値域・max_len 超過はエラー (4) docs/04 §4.10・docs/07 に 0.1 の実装範囲を固定。受け入れテスト2本追加、(1) は規則を無効化すると落ちることを確認 |
| 2026-10-04 | 4→R | 契約作成の試験 memmove: 作成契約は参照 ACCEPT・変異 REJECT(1回)。追加の報告: 境界値の直積の黙った縮小、生成入力が見えない、部分範囲の権限・配置生成・let がない |
| 2026-10-04 | R | 再設計: 直積の縮小を plan_stats.boundary_mode と limitations に出す。scope.input_summary(変数ごとの生成範囲)を追加。受け入れテスト1本、報告を消すと落ちることを確認。設計の不足4件を docs/11 に未解決として記録 |
| 2026-10-04 | R | 再設計: 領域の開始 alignment を case ごとに変える(`placement = "varied"` 既定、seed 由来で再現可能)。fixture copy_mut_unaligned(整列していない経路だけ1バイト不足)で aligned=ACCEPT / varied=REJECT を確認。全課題の参照20件 ACCEPT・変異21件 REJECT を再確認(各1回) |
| 2026-10-04 | R | docs/12 エージェント向けの使い方を追加(試験の指示文で渡していた内容と、契約作成の試験で分かった書き方を正本化) |
| 2026-10-04 | 4 | 反例から直すループを初めて観測: gcc -O3 base64 の修正、docs/12 だけを渡した条件A で REJECT → 性質IDと反例の入出力から箇所を特定 → ACCEPT。最終判定 PASS(A・B とも、各1回) |
| 2026-10-04 | R | 再設計: 違反の detail に why_false(forall の反例添字、偽の比較の両辺、添字の値、`and` は偽の側だけ)を追加。式と値の表示を120文字で打ち切る。単体テスト1本。docs/12 に recent_instructions の限界を明記 |
| 2026-10-04 | 4 | 新しい診断で再確認(RHA): PASS。対照の REJECT で why_false が正しい箇所(奇数組・ビット1)を指したと報告。docs/12 に出力の形と終了コードを追記(--gate の値は実装で確認して記載) |
| 2026-10-04 | E1 | 次の目標: 一度では正しく書けない規模で Mukoz が到達率を上げるか。課題 utf8_count(x86・aarch64)を追加: 契約は forall による位置ごとの条件(fold 不要)。生成器に重み付き断片 `pieces` を追加。gcc 参照 ACCEPT/PASS、変異4種(overlong・サロゲート・U+10FFFF超・途中切れ)は Mukoz REJECT/隠し判定 FAIL で一致(各1回)。生成ケースの妥当 UTF-8 は 246/1028 |
| 2026-10-04 | E1 | utf8_count の aarch64 参照(手書き符号化)と変異4種: Mukoz と a64.py が全一致(各1回) |
| 2026-10-04 | 4 | 較正(実行なしの一発書き、各1回): utf8_count x86 2/2・a64 2/2 PASS → 易しすぎ。utf8_to_utf16 x86 2/2 PASS・a64 1/2 PASS(不合格の1件は Mukoz も REJECT で一致) |
| 2026-10-04 | E1 | utf8_to_utf16 / utf8_to_utf16_fast(8バイト ASCII 一括処理を要件化)の契約・gcc 参照・変異を追加。生成器に ASCII の連続断片、隠し判定に ASCII 連続を追加。注意: zsh で引用なし $2 は分割されず、変異用フラグが効かない版を一度作った(cmp で検出、作り直して両判定器 REJECT を確認) |
| 2026-10-04 | 4 | 較正の続き(Opus、実行なしの一発書き、各1回): utf8_to_utf16_fast x86 2/2・a64 2/2 PASS。codec(4機能: UTF-8⇔UTF-16・base64 符号化/厳密復号、x86 1932〜1993 B・a64 1360〜1376 B)x86 2/2・a64 2/2 PASS。隠し判定と Mukoz は全件一致。x86 #2 は自作の Python 版アルゴリズムを Python のコーデックと突き合わせていた(バイナリの実行ではないが、Z 条件の「推論のみ」より強い) |
| 2026-10-04 | E1 | codec の契約(48KB、op ごとに結果・出力・残りの不変、範囲外 op)・gcc 参照・変異2件(lonelow / padbits)。padbits は最初 ACCEPT(非正規パディングの生成が希薄)→ 断片の重みを上げて REJECT(3件)。Opus では一発書きの失敗域に届かないため、較正を Haiku 4.5 に移す |
| 2026-10-04 | 4 | 較正: Sonnet 5.5 の一発書き 8/8 PASS(utf8_to_utf16・codec、x86/a64)。Haiku 4.5 は utf8_to_utf16・codec 0/8、小課題 hex_encode 1/2・memmove 0/2・utf8_count 0/2・base64 0/2 |
| 2026-10-04 | 4 | 本実験(Haiku、各条件4件): utf8_to_utf16 x86 は A/B/Z とも 0/4、a64 も判定済み分すべて FAIL(床)。memmove x86 は A 3/4・B 3/4・Z 2/4。Mukoz の判定は memmove 12件すべて隠し判定と一致。差は見えない(件数が少ない) |
| 2026-10-04 | E1 | 利用者の指示で、プロセス境界とモジュール分割を実装(13章)。最初の todo 検査で、Mukoz が argv の式の数を超える argc を黙って切り詰めていた欠陥が見つかり、BINDING_MISMATCH に直した。監視の requires 違反の位置に rsp の値を出していた誤りを、戻り先アドレスに直した |
| 2026-10-04 | 4 | 本実験5(Sonnet 5.5、ToDo CLI x86-64 raw、各条件3件): A 3/3・B 3/3・Z 3/3 が隠し判定(実機300セッション)PASS、Mukoz も全件 ACCEPT で一致。大きさ 1279〜1512 バイト、各 2.5〜3.5 分。途中の誤り: A は1件が argc の数え違いを Mukoz の反例で直した(3回目で ACCEPT)、B は2件が自前テストで符号化の誤り(jcc の opcode、cmp の即値)を直した。Z は実行なしで3件とも正しかった → この規模は Sonnet の一発書きの範囲内。到達率の差は測れない |
| 2026-10-04 | 4 | exp3 の残り2件: AArch64 の隠し判定(純 Python)が無限ループする提出物で1時間止まっていた。失敗20件で打ち切るよう直した(failed は下限) |
| 2026-10-04 | 4 | 較正 todo2(優先度・並べ替え・大小無視の検索・編集・標準入力からの取り込み、参照 3435 バイト): Sonnet の一発書き Z 3/3 PASS(2969〜3619 バイト、各 5.5〜7 分)。1件は Python で自前の命令エンコーダを書いていた(ルール上は許容) |
| 2026-10-04 | 4 | 本実験7(Haiku 4.5、todo、各条件3件): A 0/3・B 0/3・Z 0/3。全件 134〜608 バイトのスタブで止まり、ファイル入出力に届かない。Mukoz は9件とも REJECT で隠し判定と一致。Haiku の失敗は「誤り」ではなく「書き切れない」で、反例による修正ループでは越えられない。1Z は作業ディレクトリ外(リポジトリ eval/todo2)に書き込んだ(実験ディレクトリへ移した)。1B は `unshare` を付けずに起動器を呼んで chroot に失敗し、テストを諦めた |
| 2026-10-05 | E1 | モジュール版 todo を作る途中で Mukoz の欠陥を2つ直した: (1) 監視が requires を破った呼出しでも呼び先の ensures を評価し、呼び先にも責任を付けていた(`let _ = ok;` で結果を捨てていた)。受け入れテストに、前提外でだけ誤る呼び先 add64_small_only を追加(修正前のコードで落ちることを確認)。(2) 監視が契約のすべての入力の復元を要求し、生成専用の入力を持つ契約を監視できなかった → 条件が参照する変数だけに。参照実装のルーチンを -fno-pic で作り、単独(0x100000)とリンク時(0x500000)で番地がずれて fmt_line が範囲外を読んだ → 仕様に「ルーチンは位置独立」と明記 |
| 2026-10-05 | 4 | 本実験8(Haiku 4.5、todo をモジュール8個に分割、各条件3件): 隠し判定は A 0/3・B 0/3・Z 0/3。全件で main がスタブ(9〜256 バイト)、ファイル入出力に届かない。ルーチン単位の Mukoz ACCEPT は A: 0・—・2(find_rec, make_rec)、B: 0・1(make_rec)・0、Z: 0・0・0。Mukoz の検査回数 A 31・31・40。Mukoz の本体判定は判定できた8件すべてで隠し判定と一致。2A は自分で下位エージェントを起動し、その下位エージェントは 2A の報告後もファイルを書き換えていた(停止した。main.bin なし=INCOMPLETE)。分割しても Haiku の律速は main(参照で 1367 バイト)の量 |
