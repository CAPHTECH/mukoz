# 4種類の契約違反を、通常CLIで検出・再生できた

[利用者向けの実行例](../../../examples/security_checks/README.md)に、ABI破壊、モジュール間の契約違反、引数の幅・符号の誤解、禁止された作用を追加した。x86-64とAArch64の30例を通常の `mukoz check` に通し、16例の違反を `replay` でも再現した。VMとnative実行は使っていない。

| 分類 | 例数（両ISA合計） | 確認内容 |
|---|---:|---|
| ABI | 6 | 正常な加算、保存registerの正しい退避・復元、計算結果は正しいが保存registerを壊す実装 |
| モジュール境界 | 8 | 正常な結合、calleeの結果違反、calleeのABI違反、callerの前提条件違反と責任の区別 |
| 引数 | 8 | 上位bitに依存しない32 bit引数、明示的なzero/sign extension、拡張方法を誤る実装 |
| 作用 | 6 | pure routineのsystem call試行、同じprocess binaryで許可／禁止を変えたwrite |
| 未対応の対照 | 2 | 未対応命令を違反と断定せずHOLDにする |

結果はACCEPT_WITHIN_SCOPE 12例、REJECT 16例、HOLD 2例。照合エラーは0件だった。期待する判定・property・責任・許可される対照は、実行前に `examples/security_checks/cases.json` に記述した。

ABI破壊の例では、両ISAとも加算結果のpropertyは全ケースで充足した。callerの前提条件違反でも、最終の合計値は全ケースで正しかった。そのうえで別の契約違反を検出している。calleeの前提が破られた呼出しについて、calleeの結果違反に責任を転嫁しないことも確認した。

`tests/security_checks.rs` は、30例と16反例の再生をCIで実行する。`examples/security_checks/test_verifier.py` は保存した実記録を改変する11試験で、claim欠落、誤った責任、空・未実行のケース、未資格のengine、未対応命令の誤分類、反例欠落などを成功扱いしないことを確認した。

最終記録は `runs/matrix-2/` にある。各例の実行命令・CLI結果・store・replay結果と、行列・runner・Mukozのhashを保存した。圧縮した記録と集計は `../evidence/` に置き、`python3 research/security-observation/restore_evidence.py` で復元する。初回の `matrix-1` も期待どおりだったが、成功判定のclaim完全性検査を加えた後、最終行列を実行し直した。

実行方法は次のとおり。出力先は新しいディレクトリを指定する。

```sh
cargo build --release
python3 examples/security_checks/run.py /tmp/mukoz-security-new
cargo test --release --test security_checks
python3 examples/security_checks/test_verifier.py
```

検証範囲は人工的な小さいroutineとprocess、明示した契約、列挙した入力、資格確認済みの実行モデルである。実アプリ全体の認可判断、実OSに対する通信制限、完全な制御フロー保護を保証する結果ではない。ABIとモジュール監視には既存の機能を使い、符号拡張などの不足していたfixtureと、比較・回帰試験・利用手順を追加した。
