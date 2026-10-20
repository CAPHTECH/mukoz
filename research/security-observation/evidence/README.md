# 検証記録

`f11-local-2.tar.gz` は、F11のローカル検証288実行の記録一式である。環境と集計は、展開せず読めるよう `f11-local-2.env.json` と `f11-local-2.summary.json` にも保存した。

`regression-fixtures.tar.gz` は、既存の正規化器・A0/A1・E1の記録ベースの試験に必要な入力をまとめたもの。全実験の記録ではない。

`x3-2.tar.gz` は途中アクセスと最終状態の比較、`x3-native-1.tar.gz` はMemcheckによるnative対照の記録である。`x3-2.summary.json` と `x3-native-1.summary.json` に集計を保存した。

`security-checks-matrix-2.tar.gz` はABI・モジュール・引数・作用の30例と16反例の再生記録である。集計は `security-checks-matrix-2.summary.json` にある。

各アーカイブのsha256と展開されるディレクトリを `manifest.json` に記録している。試験前に次のコマンドで復元する。既存の同名ファイルはアーカイブの内容で置き換わる。

```sh
python3 research/security-observation/restore_evidence.py
python3 research/security-observation/fx/test_f11_local.py
python3 research/security-observation/normalize/test_memcheck.py
```

Python 3.12以降を使用する。復元先を引数で指定すれば別ディレクトリにも展開できる。復元したrunsとビルド成果物はGitの対象に含めない。
