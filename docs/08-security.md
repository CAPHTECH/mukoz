# 08 セキュリティと信頼境界

## 8.1 想定する脅威

AIや人が誤って作った不正な命令列・形式に加えて、入力ファイル・契約の文字列・Binding・対象の出力が悪意を持つ場合を考える。

- 無限ループ、過大なメモリ要求、ログの増幅
- パーサやエミュレータの脆弱性を突く入力
- out-of-bounds、任意の外部作用(ファイル・通信・子プロセス)
- 証跡の偽装、検査後のファイル差し替え
- 対象の出力に仕込んだ指示(prompt injection)

初期版は汎用のマルウェアsandboxを名乗らない。エミュレータ・パーサ・OS・ハードウェアの脆弱性、強い権限を持つ攻撃者を排除したとは主張しない。

## 8.2 既定の動作

- まず実行しない検査(`inspect`)を行う。
- 既定で使えるのは `emulated` だけ。`native-routine`・`native-process`・`translated-process` は、Policyで**対象のdigestを指定して**許可したものか、生成ループ用の試行区域(8.4)の条件を満たすものに限る。
- 対象にホストの HOME、認証情報、SSH agent、クラウドトークン、ソケットを渡さない。環境変数は既定で空。
- 未対応のsyscall・import・命令をホストへ逃がさない。
- スナップショットは実行用の一時領域に置き、契約・Policy・証跡への書込み権限を対象に与えない。

通常のプロセス分離と環境変数の削除だけでは、ファイルシステムや通信は遮断できない。必要な強制能力がなければ、その危険は残ったものとして扱い、Policyの判断に戻す。

## 8.3 隔離能力

能力を列挙し、そのホストで**実際に試して効いたもの**だけを返す(02章 2.5)。

```text
process_isolation
resource_limits
filesystem_restriction
network_restriction
descendant_process_control
credential_isolation
artifact_immutability
```

| OS | 候補となる仕組み | 現ホストでの確認 |
|---|---|---|
| Linux | rlimit、cgroup v2(メモリ・子孫停止)、user namespace + network namespace(通信遮断)、Landlock(ファイルシステム制限)、seccomp(syscall制限) | cgroup v2 がマウント済み、LSMに landlock がある、`unshare -Urn true` が1回成功。cgroupの委譲の有無、Landlock・seccompが実際に効くかは `[U]` |
| macOS | rlimit、プロセスグループ。sandbox系の仕組みは実機で試してから判断 | 未確認(ホストなし) |
| Windows | Job Object(資源・子孫停止)、AppContainer 等 | 未確認(ホストなし) |

- プロセスグループの終了だけで、離脱した子孫を必ず止められるとはしない。
- VMのように環境ごと破棄できる方式と、普通の子プロセス方式を同じ扱いにしない。
- MVPの必須条件は「隔離能力がないことを正しく報告する」こと。万能なsandboxの実装を出荷条件にしない。

## 8.4 生成ループでのnative実行(試行区域)

digestごとの許可だけでは、AIが新しいバイナリを作るたびに人の許可が要り、生成と検査の繰り返しが止まる。そこで、**隔離能力が確認できたときに限り**、特定の場所の成果物をまとめて許可する規則を設ける。既定では無効で、所有者が `policy.toml` に書いたときだけ働く。

```toml
[[native_trial_zones]]
id = "ai-build"
artifact_dir = "build/"                     # この下のファイルだけ。シンボリックリンクは辿らない
executors = ["native-routine", "native-process"]
targets = ["x86_64/raw/sysv-x86_64/none", "x86_64/elf/sysv-x86_64/linux"]
require_isolation = [
  "network_restriction",
  "filesystem_restriction",
  "resource_limits",
  "descendant_process_control",
  "credential_isolation",
]
max_wall_ms_per_case = 1000
```

**規則:**

1. 成果物のスナップショットが `artifact_dir` の下から読まれ、ターゲットとExecutorが一覧にあり、`require_isolation` の**すべて**がそのホストの `HostProbe` で確認済みのときだけ実行する。
2. 1つでも欠ければ実行せず、そのclaimを NOT_EVALUATED にする(理由 `NATIVE_NOT_PERMITTED`、欠けた能力の一覧付き)。emulated へ切り替えない。
3. 実行時、対象は確認済みの隔離を**すべて適用した状態**で起動する。probeで確認したことと、その回に適用したことの両方を証跡に残す。
4. 試行区域で実行した結果には、区域IDと適用した隔離を `execution_platform` に付ける。

**`native-routine` の追加条件:** ルーチンはsyscallを使わないはずなので、Linuxでは seccomp の strict モード(read・write・exit・sigreturn 以外のsyscallで終了させる)をトランポリンの直前に適用する。適用できないホストでは `native-routine` を試行区域で使えない。`[R]` strict モードは古くからある最小のseccompで、プロセスが自分に適用できる。現ホストで効くかは未確認 `[U]`。

**`native-process` の候補(Linux):** user namespace + network namespace(通信遮断)、Landlock(読める場所を対象ファイルと最小限に限定、書込み不可)、rlimit(CPU時間・メモリ・ファイルサイズ・プロセス数)、cgroup v2(子孫の停止)、空の環境変数と専用の一時HOME。どれが実際に効くかは probe で確かめる(現ホストで確認済みなのは `unshare -Urn true` の成功1回だけ)。

**残るリスク:** 隔離の仕組み自体やカーネルの脆弱性を突く対象は防げない。試行区域は「AIの誤り(暴走・誤った書込み・意図しない通信)からホストを守る」ためのもので、悪意あるコードへの防御を主張しない。所有者がこのリスクを受け入れて有効にする。

## 8.5 TOCTOU と同一ユーザー

- 計画はパスではなくスナップショット(digest)に結び付け、実行の前後で digest を検査する。
- Linuxの `native-process` は、スナップショットを memfd に置いて fd から起動することで、パスの再解決をなくす(05章 5.5)`[R]`。他のOSではコピーしたファイルのパスが再解決されるので、同じユーザーの攻撃者が差し替えられる余地を記録する。
- `.mukoz/` と `policy.toml` は論理的な分離であり、強制的な権限分離ではない。同じユーザーに任意のshellを許していれば書き換えられる。強い完全性が必要なら、別ユーザー・別環境に置く。

## 8.6 外部データと prompt injection

- 対象のstdoutに `"admission": "ACCEPT"` のような文字列や「基準を変えよ」という文が含まれても、制御情報として扱わない。
- 出力には、対象由来のデータを `untrusted` と印を付けた専用フィールドにだけ入れ、制御文字をエスケープする。
- ファイルパス、シンボル名、注釈も信頼しないデータとして扱い、出所と種類を付けて返す。

## 8.7 信頼の基盤

具体的なテストで信頼するもの: Inspector、ローダー、Binding評価、エンジン(Unicorn)またはOSのプロセス実行、作用モデル、式評価器、Assessor、OS・ハードウェア、証跡の保存。

- **生成者を信頼対象から外すことと、信頼対象がなくなることは別である。**
- 同じAIが対象と契約の両方を作ってもよいが、それだけでは独立性は得られない。契約の承認、別の参照実装との比較、手で確定した値との照合で、共通の誤解を見つける経路を持つ。
- Mukoz自身を検査する場合は、判定器の独立性を claim に記録する(06章 6.9、09章 9.6)。
