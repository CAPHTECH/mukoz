# 03 アーキテクチャとデータモデル

## 3.1 論理構成

```text
   Contract / Binding / Suite (TOML or JSON)        Artifact file
                  │                                      │
                  ▼                                      ▼
            Spec Loader ──────────────►  Artifact Store + Inspector
                  │                                      │
   Policy ────────┤          HostProbe / Qualification ──┤
                  ▼                                      ▼
                         Planner(能力照合・ケース生成・固定)
                                   │
                              Frozen Plan
                                   │
                              Supervisor
            ┌──────────────┬───────┴───────┬──────────────────┐
            ▼              ▼               ▼                  ▼
     emulated worker  native-routine  native-process   translated-process
     (Unicorn)        worker          worker           worker
            └──────────────┴───────┬───────┴──────────────────┘
                                   ▼
                     Observation → Evidence Store
                                   │
                                   ▼
                     Assertion Evaluator → Assessor
                                   │
                                   ▼
                 Claims / Counterexamples / Assessment
```

## 3.2 コンポーネント

| Component | 責務 | 持たせない責務 |
|---|---|---|
| Spec Loader | TOML/JSONの厳密な読込み、式の構文解析と型検査、正規化、digest | 自然言語から期待値を決めること |
| Artifact Store | 不変スナップショット、digest、派生関係(署名前後等) | 信頼ラベルの自動付与 |
| Inspector | 形式ごとの構造検査、入口解決、依存の検出、LoadPlan作成 | OSローダーの完全な代行 |
| Binding Validator | 入口・レジスタ・領域・ABIの整合 | Bindingを意味的に正しいと認めること |
| Host Probe | ホスト情報、Executorの能力確認、エンジン適格試験の記録 | 能力の推測・宣言値の採用 |
| Planner | 能力照合、ケース生成、予算、Planの固定 | 検査できない必須claimを黙って外すこと |
| Supervisor | workerの起動・期限・資源・クラッシュの監督 | 対象へのshell権限付与 |
| Worker | 1つのExecutorで実際に実行し、観測を返す | 契約・判定基準の変更 |
| Effect Model | 作用の意味(write/exit等)とOS×ISAごとのsyscall adapter | 未知のsyscallのホスト転送 |
| Assertion Evaluator | 観測から型付きの性質を決定的に評価 | 曖昧な評点での合否 |
| Assessor | 範囲・欠測・反例・contextに基づく採否 | リリース承認 |
| Evidence Store | 不変の証跡、索引、ページ付き射影、再現材料 | 古い結果の無条件な再利用 |

## 3.3 プロセス分離

- 対象のコードを Mukoz 本体のプロセスに読み込まない(`dlopen` やポインタ呼出しをしない)。
- エミュレータのFFI(Unicorn)も別プロセスのworkerに置く。workerがクラッシュしても本体は判定を続け、`WORKER_FAILED` を記録する。
- worker分離はクラッシュと資源の境界であり、悪意あるコードからホストを守るsandboxではない(08章)。
- 本体とworkerは版付きの構造化IPCで通信する。handshakeで worker build ID・Executor・能力を交換し、run/case ID・イベント順序・payload長を検査する。対象のstdoutをIPCに混ぜない。

メモリアクセス等の禁止監視は、性能のためworker内で逐次評価してよい。その監視コードは信頼対象に含まれる。保存traceを省いても、判定に必要な監視が最後まで続いたかを別に記録する(06章 6.5)。

## 3.4 主要オブジェクト

| Object | 主なフィールド |
|---|---|
| `ArtifactSnapshot` | digest、byte長、形式、ISA、選んだslice、依存、取得元、親artifact |
| `Contract` | schema版、境界(routine/process)、型、性質ID、正規化AST、digest |
| `Binding` | contract ID、target platform、入口、引数・結果・領域の対応、作用モデル、digest |
| `Suite` | contract/binding ID、生成器、seed、ケース数、予算、Executor、必須claim |
| `Policy` | 許可するExecutor・ホスト、必須claim、上限、native実行の許可対象digest、試行区域(08章 8.4) |
| `HostProbe` | ホストID、OS版、CPU・機能、Executorごとの確認済み能力、probe版 |
| `EngineQualification` | エンジン名・ビルド、ISA、ホスト、適格試験の版と結果 |
| `Plan` | subject context、ケース集合(回帰ケース・生成ケースの具体値)、生成器版、Executor、適用した上限 |
| `Run` | plan ID、execution platform、状態、ケースごとの観測、終了理由 |
| `Claim` | 性質ID、評価、方式、範囲、仮定、限界、独立性、証跡参照 |
| `Finding` | 反した性質、expected/observed、位置、関連trace |
| `Counterexample` | 再現用の入力・初期状態・環境応答、対象digest、failure predicate |
| `RegressionCase` | 契約digest・ターゲットに結び付いた過去の反例の入力。成果物には結び付けない(04章 4.8) |
| `Assessment` | 採否、未完了事項、適用したplatform範囲、`release_authorized = false` |

## 3.5 同一性: subject context と execution platform

0.4は1つのcontext digestにすべてを入れていた。ホスト情報を入れると別ホストの結果がすべて不一致になり、入れないとホスト差が記録されない。そこで2つに分ける。

```text
subject_context = H(
  artifact_snapshot, selected_slice,
  contract, binding, suite_semantics, environment_model,
  policy_revision, evaluator_version
)

execution_platform = {
  host_id, os_version, cpu_model, cpu_features,
  executor, engine_name, engine_build, worker_build,
  translator_name_and_version   # translated-process のみ
}
```

**規則:**

1. 採否の前提として、証跡の `subject_context` は現在の値と一致しなければならない。不一致なら HOLD(古い証跡)。
2. `execution_platform` は**照合キーではなくclaimの範囲**として扱う。claimは「どのExecutor・どのホストで得たか」を必ず持つ。
3. Policyは必要なplatform範囲を指定できる(例: 「`native-process` の結果を `linux-x86_64` で1つ以上」)。満たさなければ HOLD。
4. `emulated` の結果は、そのホスト・エンジンビルドの `EngineQualification` が有効なときだけ採用する。適格記録がないホストの結果は HOLD。
5. 別ホストで得た結果同士を黙って1つにまとめない。結果が食い違えば `PLATFORM_DIVERGENCE` を記録する(06章 6.8)。

Planは `subject_context` とケース集合から別の内容IDを持つ。seed・生成器版・ケース順序が変われば別Planになる。

ASLR後の実アドレスはrunごとの情報とし、永続的な識別子に使わない。場所は `artifact + slice + file offset / image相対位置` で表す。

## 3.6 署名と配布物

署名(Mach-Oのコード署名、PEのAuthenticode等)の前後で、ファイルは別の `ArtifactSnapshot` になる。`.text` が同じでも、ファイル全体の結果を再利用しない。入口・ロード情報・データ・依存が変われば振る舞いも変わり得るためである。署名は所有者がMukozの外で行い、署名後のファイルを新しく登録する。

## 3.7 値の表現

- 64bitのレジスタ値・アドレスを JSON number にしない。bv値は幅に応じた固定桁の小文字hex文字列(`"0x00000000000000ff"`)とする。
- stdout/stderr はバイト列が正本。UTF-8 は表示用の射影で、不正UTF-8・NUL・改行を正規化しない。
- サイズ・件数は上限を決めた整数とする。
