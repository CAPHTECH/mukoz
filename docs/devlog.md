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
