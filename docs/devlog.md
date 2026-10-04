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
