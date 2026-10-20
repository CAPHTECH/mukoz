# X3：最終状態が同じでも、呼出しのアクセス権限への違反を発見できる

Mukozの既存emulated executorを使い、正常なコピーと最終状態が同じになる許可外アクセスを検出した。利用者向けの契約・Binding・実行例は [examples/security_access](../../../examples/security_access/README.md) に追加した。Memcheckの診断を取り込むF11とは別に、Mukozの命令ごとのアクセス検査を使う。

## 対象と結果

8 byteの入力を出力へ複写してstatus 0を返す、x86-64とAArch64のraw routineを対象とした。入力領域は読取り専用、audit領域は書込み専用である。正常版のほか、auditの読取り、入力の一時書換えと復元、入力範囲の1 byte後の読取り、許可されたstackの利用を用意した。

| 成果物 | 最終状態だけの比較 | 通常のmukoz check（両ISA） |
|---|---|---|
| 正常コピー | 正常版と一致 | 24/24充足 |
| 書込み専用auditの読取り | 正常版と一致 | 24/24メモリ違反 |
| 入力を書き換えて復元 | 正常版と一致 | 24/24メモリ違反 |
| 入力範囲の1 byte後の読取り | 正常版と一致 | 24/24メモリ違反 |
| 許可されたstackの利用 | 正常版と一致 | 24/24充足 |

`run.py` は、同じコード・初期register・初期memoryに対して、広くアクセスを許す対照実行と、呼出しの権限を適用した実行を対にした。両ISA×5成果物×24入力×2条件の480実行で、対照側の最終general register・flags・data page全体・stack page全体が正常版と一致した。code pageは成果物ごとに異なるため比較に含めない。

対照実行はエミュレータのprotocolを使う機序確認であり、Mukozによる安全性の承認ではない。通常CLIでの検証は別に行い、必須claimを外さず、両ISAの3違反版をすべてREJECT、正常版とstack版をACCEPT_WITHIN_SCOPEとした。6組の違反について、保存した反例をreplayし、`machine.memory.access` のVIOLATEDを再現した。許可外アクセスで止まる実行の機能propertyはINCONCLUSIVEのままである。

`native_control.py` では、同じx86-64のassemblyをCの呼出し側へリンクし、三つのwordを一つの有効なallocationへ配置した。3入力×5成果物の15実行で、最終の値は期待どおり、Memcheckのエラーは0件だった。入力範囲の直後も、この対照では同じallocationの次のwordである。allocatorの有効範囲にある操作でも、呼出し側が指定する読取り・書込みの制約には違反し得ることを示す。

## 再実行

```sh
cargo build --release
python3 research/security-observation/x3/run.py /tmp/mukoz-x3-new
cargo test --release --test security_access
```

出力先は新しいディレクトリを指定する。既存エミュレータを直接使う対照実行、通常CLI、反例の再生まで行う。VMやnative実行は使わない。`--mukoz`・`--engine` で実行ファイルを指定できる。

Memcheckを使ったnative対照は、x86-64ホストで別途実行する。

```sh
python3 research/security-observation/x3/native_control.py /tmp/mukoz-x3-native-new
```

この対照にはGCCとMemcheckが必要。`--valgrind`・`--debug` でツールと追加デバッグ情報の検索先を指定できる。

## 証拠と限界

最終実行は `runs/x3-2/`、native対照は `runs/native-1/`。集計、各CLIの判定とstore、replay結果、protocolの入出力、コード・実行器のhashを保存した。公開する記録は `../evidence/` に圧縮して保存する。

通常CLIの資格試験を通した同じエミュレータで、対照実行と検証を行う。これは同じ実行器を使った比較であり、実機との同値性の証明ではない。Memcheck対照はx86-64の3入力だけで、emulated側とはmemoryの配置が異なる。計測したのは検出の有無で、費用の比較はしていない。

DESIGN.md 15.3の案に対し、今回の実装は共通の全アクセスtraceからのprojectionではなく、固定した初期状態での対実行である。最初のCLI試験では、代表findingの件数を実行数と取り違えたassertionが失敗した。集約claimの `cases_violated` を使うよう直した。実行エンジンの変更は行っていない。

この実験が示す価値は、呼出しのアクセス範囲を明示すれば、正常な結果だけでは分からない途中の違反を、ソースやdebug情報なしで検査・再生できることである。一般的な認可判断や実アプリケーション全体の安全性を確認した結果ではない。
