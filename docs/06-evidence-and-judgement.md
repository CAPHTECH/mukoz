# 06 証跡と判定

## 6.1 検査の4分類

| 分類 | 例 |
|---|---|
| Artifact | header整合、入口の範囲、依存の解決 |
| Machine | ABIの保存規則、SP、制御移行、メモリアクセス、予算 |
| Effect | stdout/stderr、exit、禁止作用の試行、作用の順序 |
| Semantic | 加算結果、状態の更新、エラー結果、frame |

Semantic は Binding を介して観測する。Machine が通っても Semantic の代わりにはならない。

## 6.2 メモリの条件

次は別々の条件として扱う。

```text
mapped           エンジンがアクセスできる
owned            契約上、その領域が誰のものか
initialized      初期値が決まっている
allowed_read     読んでよい
allowed_write    実行中に書いてよい
unchanged_at_end 終了時に開始時と等しくなければならない
```

- 対象外の領域へ書いてから元に戻す動作を禁止したいなら、終了時の比較ではなく全書込みの監視が必要。
- アクセス幅を含む `[addr, addr+width)` が許可領域に完全に収まることを確認する。アドレス加算のoverflow、領域をまたぐアクセスも扱う。
- 未初期化領域の読込みを契約で禁止するなら policy 違反として検出する。任意の初期値を許すなら、その値を入力として変化させる。ランダムな初期値での成功を、全初期値での成功としない。

## 6.3 作用の観測と強制

能力ごとに、**観測できるか**と**強制的に止められるか**を分けて持つ。

| 能力 | emulated(ルーチン) | emulated(プロセス+作用モデル) | native-routine | native-process |
|---|---|---|---|---|
| レジスタ | 観測 | 観測 | 入口・出口のみ観測 | 不可 |
| メモリアクセス | 全アクセスを観測・強制 | 同左 | ページ単位の強制のみ | 不可 |
| stdout/stderr | 作用は禁止 | 仮想バイト列 | 作用は不可 | 実バイト列 |
| syscall試行 | 観測・強制 | 許可分をモデル化、他は停止 | 初期は不可 | 初期は不可 |
| 通信の不在 | モデル内で確認 | 許可以外を停止 | 断定不可 | 断定不可(隔離能力があれば強制のみ) |
| ホストの保護 | worker分離だけでは保証しない | 同左 | 同左 | 隔離能力が必要(08章) |

「通信を試みてはならない」と「通信が外に出ない」は別の性質である。隔離で送信を止めても前者の違反は起こり得る。また、隔離で作用を止めたことで対象の振る舞いが変わることがあるので、隔離環境の結果を無制限な環境へ一般化しない。

## 6.4 Claim の軸

結果を1つのPASSにまとめない。性質ごとに次を持つ。

| 軸 | 値 | 意味 |
|---|---|---|
| `evaluation` | `SATISFIED_IN_SCOPE` `VIOLATED` `INCONCLUSIVE` `NOT_EVALUATED` | その性質について分かったこと |
| `method` | `structure` `example` `property` `differential` | 検査方式 |
| `execution` | `completed` `budget_exhausted` `worker_failed` `blocked` `unsupported` | 実行の終わり方 |
| `platform` | Executor・ホスト・エンジン(03章 3.5) | どこで得たか |
| `independence` | `independent` `previous_version` `self` | 判定器と対象の関係(6.9) |
| `context_match` | `true` / `false` | 現在の subject context に当てはまるか |

形式検証の値(`exhaustive_within_domain` `solver_checked` 等)はデータモデルに予約するが、実装されるまで出力しない。

## 6.5 監視とtraceの欠落

```text
monitoring_complete      判定に必要な監視を最後まで行えたか
trace_storage_truncated  保存した詳細traceに省略があるか
response_truncated       今回の応答が一部だけか
```

表示用traceを省いても、監視がすべて行われていれば判定は保てる。監視イベント自体を落とした場合は、該当claimを INCONCLUSIVE にする。

## 6.6 採否(Assessment)

```text
if subject_context が現在と一致しない:
    HOLD (STALE_CONTEXT)
else if 現contextで有効な、必須claimの反例がある:
    REJECT
else if 必須claimに NOT_EVALUATED / INCONCLUSIVE / 欠測 / 未完了がある:
    HOLD
else if Policy が要求する platform 範囲を満たさない:
    HOLD (PLATFORM_SCOPE_UNMET)
else if 有効ケースが0、または必要な状況に到達していない:
    HOLD (VACUOUS_SCOPE)
else:
    ACCEPT_WITHIN_SCOPE
```

- 後続ケースのworker障害が、すでに得た反例を消すことはない。
- `--fail-fast`(07章)で最初の反例の後に残りのケースを実行しなかった場合も、REJECT は有効である。REJECT に必要なのは現contextで有効な反例1つだからである。ただし未実行のケース数を `scope` に出し、「他の性質は満たしていた」とは書かない。
- 古いartifactの反例を、新しいartifactの REJECT に使わない。新しいrunで再現させる。
- 未知のclaim種別・評価値を受け取ったクライアントは HOLD とみなす(推測でPASSにしない)。
- `release_authorized` は常に `false`。

## 6.7 時間切れと異常終了

| 状況 | 扱い |
|---|---|
| 検査の実行予算を使い切った | INCONCLUSIVE → HOLD |
| 契約に書いた「N命令以内」を超えた | その性質への VIOLATED |
| 契約に書いた実時間制限を超えた | その測定条件での違反。測定誤差・再現性を併記 |
| workerがクラッシュした | `WORKER_FAILED`。対象の異常と同一視しない |
| 対象が禁止されたメモリアクセスで止まった | 有効な初期状態・対応モデルでの禁止動作なら反例 |

`must_return` の契約で一定時間戻らなかったことを、数学的な非停止の証明とはしない。

## 6.8 差分試験

同じケースを複数のExecutor・ホストで実行し、観測を比べる。

| 組み合わせ | 得られるもの |
|---|---|
| `emulated` と `native-routine`(同ISAホスト) | エミュレータと実CPUの命令意味の差 |
| `emulated` と `native-process` | 作用モデルと実OSの差 |
| 別ホストの `emulated` 同士 | ホスト・エンジンビルドによる差 |
| Contract の参照計算と対象 | 本来の検査 |

- 一致は双方の正しさを証明しない。
- UnicornはQEMU由来なので、UnicornとQEMU(qemu-user)の一致を独立した2実装の一致として扱わない。
- 不一致の原因(対象、Binding、ABI、作用モデル、エンジン)は不明なので、まず `BACKEND_DIVERGENCE` / `PLATFORM_DIVERGENCE` として、調べられる証跡を返す。

## 6.9 判定器の独立性

自己検査(09章 9.6)を扱うため、claimに `independence` を持たせる。

| 値 | 意味 |
|---|---|
| `independent` | 判定に使った式評価器・エンジンが、対象と別の実装 |
| `previous_version` | 対象と同じソース系列だが、別の(前の)版の判定器 |
| `self` | 判定器と対象が同じソース・同じ版から作られている |

- `self` の claim だけで ACCEPT_WITHIN_SCOPE を出すことは、Policyで明示した場合に限る。既定では、`self` だけの必須claimは HOLD にする。
- 式評価器自体を検査する場合、期待値をその評価器で計算すると循環になる。期待値は、実CPUの命令結果(native-routine / emulated)、手で確定した値の表、別実装のどれかから取る。

## 6.10 反例・縮小・再実行

反例は少なくとも次を持つ。

```text
artifact / subject context / plan / case のID
性質ID と failure predicate
具体的な初期レジスタ・メモリ・入力
環境応答のスクリプト
停止理由、PC、直近のtrace、作用
expected / observed
execution_platform
縮小の探索範囲と完了状態
```

- 縮小では入力値・バッファ・入力列・環境スクリプトを小さくする。Contract・Bindingの意味・期待値を書き換えて失敗を消さない。事前条件と failure predicate を保つ。
- 縮小の初期実装は、各入力を0・境界値・短いバイト列へ近づける単純な探索とする。
- 修正後のartifactで反例が再現しなかったことは「この反例への回帰テストが通った」であり、全体の正しさではない。

## 6.11 証跡の格納

```text
.mukoz/
  objects/sha256/…     # 不変: artifact, contract, binding, suite, plan, run, claim
  host/…               # HostProbe, EngineQualification
  indexes/…            # 派生索引。失っても objects から作り直せる
  work/…               # workerの一時領域
policy.toml            # 所有者が書く方針(任意)
```

- SQLサーバや分散ストレージは要らない。content-addressed なファイルを正本とし、索引は派生物とする。
- ファイルの作成は一時ファイルへの書込みと rename による置換で行い、途中のクラッシュで壊れた objects を残さない。rename・ロックの意味はOSで違うので、Store の実装をOS別に試験する(09章)。
- object 名は小文字hexのdigestだけにし、大文字小文字を区別しないファイルシステム(macOS・Windowsの既定)でも衝突しないようにする。
- 証跡に暗号学的な改竄耐性はない。同じユーザーの権限で動くプロセスは `.mukoz/` を書き換えられる(08章)。
- 全ケースのゲストメモリを丸ごと保存しない。不変ページの共有、初期データと変更ページの記録で容量を抑える。ただし再実行に必要な具体値は必ず復元できるようにする。
- 証跡容量が足りなくなったら、反例・判定材料を失ったまま成功を返さない。

## 6.12 修正のための診断情報

AIが機械語を直すには、「どの命令で何が起きたか」が最も役に立つ。finding と反例に、判定とは別の**診断情報**を付ける。

| 項目 | 内容 | 取得方法 |
|---|---|---|
| 失敗地点 | 停止したPC、artifact内のoffset、停止理由(禁止アクセス、未対応命令、予算切れ、復帰先違反等) | 監視が記録 |
| 直近の実行命令 | 停止前に実行した命令を最大64個。アドレス・offset・命令bytes・逆アセンブル | 常時動かす小さなPCリングバッファ(64件)。x86_64のような可変長命令でも、実行したPCから逆アセンブルするので命令境界を誤らない |
| 違反したアクセス | アドレス、幅、読み書き、どの領域の外か、許可領域の一覧 | メモリ監視 |
| レジスタの差 | 入口と出口(または停止時)で値が変わったレジスタ。ABI違反ならどのレジスタか | 入口・出口のスナップショット |
| 値の食い違い | 性質ID、評価した式、各部分式の値(expected/observed)。例: `result.value = 0x…`、`input.a + input.b = 0x…` | 式評価器が部分式の値を残す |
| 通った経路 | 戻り値が誤っている(停止していない)場合、そのケースで実行した命令の一覧(重複は回数にまとめる、最大256命令) | 実行trace。trace予算を超えたら省略し、省略したことを示す |
| 差分試験の最初の分岐点 | Executor間で結果が違う場合、最初に状態が食い違った命令(取れる場合) | 両方のtrace |

- 診断情報は**判定に使わない**。逆アセンブルには Capstone を使うが、表示用であり、命令の意味の根拠にしない。Capstone が使えない環境では、逆アセンブル欄を省き、省いた理由を書く。命令bytesは常に出す。
- 診断情報の大きさは finding ごとに上限(既定16 KiB)を持ち、超えたら `show` で段階的に取り出す。
- `mukoz show <finding-id> --disasm` で、失敗地点の前後を artifact から静的に逆アセンブルした結果も取れる。これは実行したPCに基づかないので、命令境界が誤り得ることを表示に明記する。

## 6.13 AI・人向けの射影

- 既定の出力は小さな要約と上位の finding だけにする。
- 詳細は run / case / property / PC / イベント種別を指定して段階的に取り出す。ページには `total / returned / next_offset / truncated` を付ける。
- 反例をたどるための安定IDを返す。古い巨大ログを会話へ貼り直す必要をなくす。
