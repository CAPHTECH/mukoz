# 07 インターフェース

## 7.1 方針

- MVPのインターフェースは **JSONを出力するCLI** だけにする。AIエージェントも人もCIも同じCLIを使う。
- 全コマンドが同じJSON封筒(7.3)を返す。人向けの表示は `--format text` で同じデータから作る。採否のロジックを表示側に持たせない。
- JSON-RPC(stdio)や MCP は、CLIと同じ操作を別のtransportで出す後段の拡張にする(10章)。
- CLIから、任意のshell実行、任意のホストパスの実行、Policyの変更、署名・OS設定の変更、デプロイはできない。
- LLMのSDKやAPIキーを依存に持たない。

## 7.2 コマンド

| コマンド | 内容 | 対象を実行するか |
|---|---|---|
| `mukoz platform probe` | ホスト情報と各Executorの能力を調べて保存 | 能力確認のための試験コードのみ |
| `mukoz platform qualify --isa <isa>` | エンジン適格試験を実行し `EngineQualification` を保存 | 試験コードのみ |
| `mukoz platform show` | 保存済みのホスト情報・能力・適格記録を表示 | しない |
| `mukoz inspect <file>` | スナップショットを登録し、形式検査の結果と LoadPlan を返す | しない |
| `mukoz check <suite.toml> [--artifact <file>] [--fail-fast]` | 読込み → 計画 → 実行 → 評価 → 採否を一度に行う | する |
| `mukoz plan <suite.toml> [--artifact <file>]` | Planを作って固定し、IDを返す | しない |
| `mukoz run <plan-id>` | 固定したPlanを実行する | する |
| `mukoz assess <run-id>` | 現在の subject context に対して採否を出し直す | しない |
| `mukoz show <id> [--page N] [--disasm]` | claim / finding / case / 反例 / trace / 診断情報の部分取得 | しない |
| `mukoz replay <counterexample-id>` | 反例を再実行する(対象が変わっていれば新contextでの診断) | する |
| `mukoz shrink <counterexample-id>` | 性質と前提を保ったまま反例を縮小する | する |
| `mukoz regressions list <suite.toml>` | その契約・ターゲットの回帰ケースを一覧 | しない |
| `mukoz regressions prune <suite.toml> --case <id>…` | 回帰ケースを明示的に外す。外した操作を記録する | しない |
| `mukoz schema list` / `schema print <id>` | 入出力のschemaを表示 | しない |

`check` は `plan` と `run` と `assess` の合成であり、別の判定ロジックを持たない。

**オプション:**

- `--artifact <file>`: Suiteの `[artifact] path` を上書きする。生成のたびにファイル名が変わる場合に使う(04章 4.7)。
- `--fail-fast`: 最初の反例が出たら残りのケースを実行せずに終える。回帰ケースから先に実行するので、再発はすぐ分かる。結果は REJECT として有効だが、未実行ケース数を出す(06章 6.6)。ACCEPT_WITHIN_SCOPE には全ケースの実行が必要なので、合格を確かめるときは付けない。

## 7.3 出力の封筒

```json
{
  "api_version": "mukoz/1",
  "command": "check",
  "ok": true,
  "data": {
    "run_id": "run_…",
    "execution": "completed",
    "assessment": {
      "admission": "ACCEPT_WITHIN_SCOPE",
      "context_match": true,
      "release_authorized": false,
      "scope": {
        "contract": "arith.add64",
        "target": "x86_64/raw/sysv-x86_64/none",
        "platforms": [
          { "executor": "emulated", "host": "linux-x86_64", "engine": "unicorn …" }
        ],
        "quantification": "enumerated_cases_not_exhaustive",
        "cases_planned": 4160,
        "cases_completed": 4160,
        "cases_failed": 0,
        "missing_required_claims": 0
      },
      "limitations": ["not_all_bv64_pairs", "not_native_execution"]
    },
    "claims_preview": { "total": 6, "returned": 1, "next_offset": 1, "truncated": true, "items": [] }
  },
  "errors": []
}
```

これは説明用の形であり、実測した出力ではない。

- `ok: true` は「操作が完了した」という意味で、検査の成功ではない。採否は `data.assessment.admission` を読む。対象が REJECT でも、検査が正しく終わっていれば `ok: true`。
- 対象の出力(stdout等)は、エスケープした上で `untrusted` と印を付けたフィールドにだけ入れる(08章 8.6)。

## 7.4 終了コード

| 終了コード | 意味 |
|---|---|
| 0 | 操作が完了した(既定。採否には関係しない) |
| 2 | 使い方・入力の誤り |
| 3 | Mukoz内部のエラー |
| 4 | 証跡の保存に失敗した |

CIで採否を終了コードにしたい場合は `--gate` を付ける。そのときだけ、ACCEPT_WITHIN_SCOPE = 0、HOLD = 10、REJECT = 11 を返す。`--gate` なしで採否と終了コードを混ぜない。

## 7.5 生成と検査の繰り返し

AIがバイナリを生成し、Mukozで検査し、結果を見て直す、という繰り返しを想定した手順。

```text
1. 準備(人またはAI): Contract・Binding・Suite を書く。Bindingに成果物のパスは書かない
2. 生成(AI):         build/add64.bin を作る(Mukozの外)
3. 検査:             mukoz check suites/add64.toml --artifact build/add64.bin --fail-fast
4. 結果を読む:       data.assessment.admission を見る
     REJECT  → 5へ
     HOLD    → 理由を見る。未対応・能力不足なら生成方針か Suite/Policy の問題。対象の誤りとして直さない
     ACCEPT  → 6へ
5. 診断:             mukoz show <finding-id>          失敗地点・直近の命令・違反アクセス・値の食い違い
                     mukoz shrink <counterexample-id>  必要なら反例を小さくする
                     → 2へ戻って直す。反例は回帰ケースとして自動で次の検査に入る
6. 確認:             mukoz check suites/add64.toml --artifact build/add64.bin   (--fail-fast なし)
                     ACCEPT_WITHIN_SCOPE なら、その範囲で合格。limitations を読んで範囲を確かめる
```

- 修正のたびに `check` をやり直す。AIの記憶に「このバイナリは通った」と残して判定を省かない。
- エージェントが保持するのは、Suiteのパス、成果物のパス、直前の run ID、注目している finding / 反例のIDで足りる。digest を推測で組み立てない。
- 回帰ケースは成果物ではなく契約に結び付くので、新しいバイナリにも自動で当たる(04章 4.8)。
- native実行を繰り返しに含めるには、所有者が試行区域を設定し、隔離能力が確認できている必要がある(08章 8.4)。設定がなければ emulated だけで回る。
- 繰り返しの回数・時間の上限はMukozの外(エージェント側)で決める。Mukozは1回ごとの検査に上限を持つ。

## 7.6 上限

| 対象 | 初期上限 |
|---|---|
| Artifact | 64 MiB |
| 式 | 深さ64、合計8,192ノード |
| Plan | 8,192ケース |
| ゲストメモリ | 1ケース16 MiB(巨大な仮想領域を実際に割り当てない) |
| 詳細trace | 1ケース4 MiB、run合計256 MiB |
| 要約出力 | 8 KiB を目標 |
| ページ | 最大32項目、64 KiB |
| stdout/stderr | 1ケース各1 MiB(Suiteで小さくできる) |

上限は Policy でさらに下げられる。実行中に引き上げることはできない。期限切れで未実行のケースが残れば HOLD。無期限の実行を既定にしない。

## 7.7 互換性

- `mukoz/1` は出力封筒の版。Contract・Binding・Suite・証跡の schema 版はそれぞれ別に持つ。
- 出力の型を変えたら schema を公開する。破壊的な変更は新しい版にする。
- 0.4 が想定した旧Mukoz v0.3 の12操作との対応は、旧実装のコードとschemaを確認してから決める(このリポジトリには存在しないため未確認)。
