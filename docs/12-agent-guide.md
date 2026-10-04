# 12. エージェント向けの使い方(mukoz 0.1)

AIエージェントが、ソースなしでバイナリを生成・修正し、契約を満たすかを Mukoz で確かめるための手順。実装済みの範囲は 04章 §4.10・07章 §7.2。食い違えば実装が正しい。

## 12.1 生成・修正のループ

```text
1. contract.toml を読む          何が確かめられるか(ensures・frame・requires)はここに全部ある
2. バイナリを作る / 直す
3. mukoz check suite.toml --artifact <file> --store .mukoz
4. data.assessment.admission を見る
   ACCEPT_WITHIN_SCOPE → 終わり(範囲は scope と limitations に書いてある)
   REJECT             → 12.2 へ
   HOLD               → 12.3 へ
5. 2 に戻る
```

- 出力は常に `{api_version, command, ok, data, errors[{code,message}]}`。**使い方・入力の誤り(ファイルがない、契約の型エラー等)では `ok: false`・`data: null`** で、理由は `errors` にある。このときは何も実行されず、ストアにも何も加わらない。終了コードは 0(判定が出た)/ 2(使い方・入力の誤り)/ 4(ストアを開けない)。`--gate` を付けると ACCEPT_WITHIN_SCOPE=0・HOLD=10・REJECT=11
- 速く回すときは `--fail-fast`。ただし合格の確認は付けずに行う(付けると ACCEPT にならない)。
- 以前の反例は回帰ケースとして自動で先に実行される(同じ `--store` を使い続ける)。

## 12.2 REJECT の読み方

| 見る所 | 内容 |
|---|---|
| `data.assessment.reasons` | 破れた性質の一覧(`VIOLATED: <性質ID>`) |
| `data.findings[]` | 性質ごとに最大3件の反例。入力(`inputs`)、停止理由(`stop`)、観測値、直前16命令(オフセットとバイト列)、`detail` |
| `detail.why_false` | **まずここを読む。**式が偽になった理由: `forall` の最初の反例の添字(`witness`)、偽の比較の両辺の値(観測値と期待値)、添字アクセスの添字の値。`and` は偽の側だけを辿る |
| `recent_instructions` | 停止した時点の直前16命令。メモリ違反・不正命令ではその場所を指す。**結果の誤り(ensures)では戻る直前の命令しか写らない**ので、`why_false` と入力から場所を絞る |
| `mukoz show <反例ID> --store .mukoz` | 反例の全情報 |
| `mukoz replay <反例ID> --artifact <file> --store .mukoz` | 直したバイナリでその反例だけを再実行。`property_now` で今どうなったかが分かる |

性質の種類と、まず疑う所:

| 性質 | 意味 | まず疑う所 |
|---|---|---|
| `<契約>/<ensures ID>` | 結果が契約の式を満たさない | 部分式の値と入力を比べる |
| `<契約>/frame.<状態>` | 変えてはいけない状態が変わった | 書き込みの範囲 |
| `machine.memory.access` | 許可された領域の外へのアクセス | 終端・オフバイワン・幅の広すぎる読み込み |
| `machine.returned` | 正しい SP で呼出し元へ戻らなかった | push/pop の数、ret の経路、未定義命令 |
| `machine.abi.callee_saved` | 保存すべきレジスタが変わった | rbx・rbp・r12–r15(x86)、x19–x29(aarch64) |
| `machine.abi.flags` | x86 の DF が立ったまま戻った | `std` の後の `cld` |
| `effects.no_forbidden` | システムコール等の禁止作用 | `syscall` / `svc` |

狭い引数(8/16/32 ビット)の上位ビットはゴミで埋めて渡される。`cmp` の幅を誤ると、ここで REJECT になる。

## 12.3 HOLD の読み方

HOLD は「合格とも不合格とも言えない」。直すべき所は `reasons` に出る。

| reasons | 意味 | 対処 |
|---|---|---|
| `INCONCLUSIVE: ... BUDGET_EXHAUSTED` | 命令数の上限に達した(停止しない可能性) | ループの終了条件。または Suite の `instructions_per_case` |
| `INCONCLUSIVE: ... UNSUPPORTED_DURING_RUN` / `ENGINE_ERROR` / `TIMEOUT` | 実行器が扱えない命令(未定義命令を含む)・実行器の失敗・時間切れ | 命令の符号化と選択を見直す |
| `VACUOUS_SCOPE` | `requires` を満たすケースが0件 | Suite の生成器(12.4) |
| `LOW_ADMITTED_CASES` | `requires` がほとんどのケースを捨てた | Suite の生成器(12.4) |

## 12.4 契約・Suite を書くとき

- まず `mukoz expr check '<式>'` で構文を確かめる(型は契約の読込みで検査される)。整数は `bv64(...)` のように幅を明示する。
- **依存する入力は生成器で作る。**`requires` で捨てるのではなく、`[generate.vars.<名前>]` の `len`(bytes の長さ)・`max`(bv の上限)で、前の変数に依存させる。例: `len = "len(input.src) + len(input.src)"`。
- 境界にしたい値は `values` に足す。
- **何が生成されたかは `data.assessment.scope.input_summary` で見る**(変数ごとの範囲・異なる値の数・0 や空の件数)。`plan_stats` には境界値の生成方式(`boundary_mode`)と、`requires` で捨てた件数が出る。
- 領域の開始位置は既定でケースごとにずれる(`placement = "varied"`)。整列を仮定するコードはここで落ちる。
- 書いた契約は、正しいと分かっている実装で ACCEPT、わざと壊した実装で REJECT になることを確かめる。

## 12.5 範囲の読み方

ACCEPT_WITHIN_SCOPE は「列挙したケースで、エミュレータ上で、書いた性質が破れなかった」という意味で、正しさの証明ではない。`limitations` に範囲の限界が出る(例: `enumerated_cases_not_exhaustive`、`emulated_only_not_native_execution`、`requires_excluded_*`、`boundary_product_reduced_*`)。`release_authorized` は常に false。
