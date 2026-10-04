# 05 成果物の検査と実行

## 5.1 Inspector

検査は必ず不変スナップショットに対して行う。形式ごとのモジュールが、共通の `LoadPlan` を作る。

```text
サイズ上限 → 形式判定 → header → ISA/slice → ロード情報(segment等)
          → 範囲・整数overflow → 入口解決 → 依存・再配置の検出
          → target platform との整合 → LoadPlan
```

`LoadPlan` は形式に依存しない: 配置する領域(仮想アドレス、サイズ、file由来のbytes範囲、zero-fill範囲、権限)、入口の仮想アドレス、未解決の依存一覧、検出した未対応機能。

### 形式ごとの検査

| 形式 | 主な検査 | 入口 | 初期の対応範囲 |
|---|---|---|---|
| raw | なし(Bindingが仮想baseと入口offsetを与える) | Bindingのoffset | 対応 |
| ELF | ident・class・endian・machine、program header の範囲と重なり、`p_filesz ≤ p_memsz`、`PT_INTERP`・`PT_DYNAMIC` の有無 | `e_entry`(仮想アドレス) | `ET_EXEC` の静的実行ファイル。`PT_INTERP` があれば `UNRESOLVED_DEPENDENCY` |
| Mach-O | magic・CPU type、load command の `cmdsize` と総長、segment の file/vm範囲、`__PAGEZERO`、dylib依存 | `LC_MAIN.entryoff`(`__TEXT` からのfile offset。仮想アドレスではない)または `LC_UNIXTHREAD` | 静的に近い単純な実行ファイル。dyld依存があれば `UNRESOLVED_DEPENDENCY` |
| PE | DOS/PE header、section の範囲、import table | `AddressOfEntryPoint`(RVA) | 初期は構造検査のみ。実行は後段 |

- 形式の読取りには `object` crate を候補とする。ライブラリが読めたことを、検査の完了とはしない。整合の検査は Mukoz 側で行う。
- `__PAGEZERO` のような巨大な仮想領域を実メモリに割り当てない。ゲストメモリ上限は配置前に検査する。
- 署名領域は範囲だけ検査する。暗号学的な署名検証やOSの配布物判定とは別。

### 逆アセンブルは入口から

`.text` の先頭から直線的に逆アセンブルしてコード範囲を決めない(命令と文字列が同じsectionに混在する例がある)。x86_64 は命令長が可変なので、とくに入口・宣言されたcode領域・実際に到達したPCを起点にする。Capstoneは表示と補助に使い、実行意味の根拠にしない。

## 5.2 実行のライフサイクル

```text
Created → Prepared → Running → Completed
                        ├──→ BudgetExhausted
                        ├──→ Cancelled
                        ├──→ WorkerFailed
                        └──→ UnsupportedDuringRun
```

`Completed` は処理が終わったという意味で、合否ではない。対象の異常終了と、検査器の異常終了を分けて記録する。

ケースごとにゲスト状態を初期化する。高速化のためのスナップショット復元は、レジスタ・メモリ・フラグ・作用モデル・エンジンの翻訳キャッシュが前のケースから漏れないことを試験してから入れる。

## 5.3 ルーチンの呼出しと完了

### 手順(全ISA共通)

1. LoadPlan と Binding の領域を配置し、権限を設定する。領域の間に guard を置く。
2. 入力からレジスタ・領域を初期化する。
3. 復帰先 sentinel と SP を ISA・ABI の規則で用意する(下表)。
4. Bindingが指定しないレジスタは、ABIに反しない範囲で seed 固定の値にする。**すべて0にしない**(0初期化に依存した誤りを見逃すため)。
5. 入口スナップショットを取り、ポインタ引数の基準値を固定する。
6. 命令・メモリアクセス・trap・予算を監視して実行する。
7. 復帰時のスナップショットと作用記録から、Contract と machine claim を評価する。

レジスタ・メモリの具体的な初期値はケースの一部として保存する。再現は seed だけに頼らない。

### 復帰先の用意と完了判定

| ISA | 復帰先の用意 | 完了の条件 |
|---|---|---|
| aarch64 | `x30`(LR)= sentinel、SP は16byte境界 | PC = sentinel、かつ SP = 入口時のSP |
| x86_64 | sentinel をスタックに積む。入口時に `rsp ≡ 8 (mod 16)`(呼出し直後の状態) | PC = sentinel、かつ RSP = 入口時のRSP + 8 |

- 単純な「最初の `ret` で終了」はしない。途中で呼んだ別ルーチンの `ret` で終わってしまうため。
- sentinel は実行不可の専用アドレスとし、そこへの到達をエンジンのフックで捕捉する。
- 復帰先以外へ制御が移ったら(code領域外へのジャンプ等)、追跡を打ち切って成功にしない。契約上の禁止なら反例、未対応の制御移行なら `UNSUPPORTED_DURING_RUN`。

## 5.4 ABI表

ABIは版付きのデータとして持つ。ISA・ABI・OSの版を別々に記録する。

| ABI | 引数(整数) | 戻り値 | 保存(callee-saved) | スタック | その他 |
|---|---|---|---|---|---|
| `sysv-x86_64` | rdi, rsi, rdx, rcx, r8, r9 | rax(, rdx) | rbx, rbp, r12–r15, rsp | 呼出し時に16byte境界。RSP下の128byte(red zone)は書込み可 | 入口・出口で DF=0 |
| `win64` | rcx, rdx, r8, r9 | rax | rbx, rbp, rdi, rsi, r12–r15, rsp, xmm6–xmm15 | 16byte境界。呼出し側が32byteのshadow spaceを確保。red zoneなし | — |
| `aapcs64` | x0–x7 | x0(, x1) | x19–x28, x29, SP、v8–v15の下位64bit | SPは常に16byte境界 | x18 はプラットフォームレジスタ(用途はOSが決める) |
| `apple-arm64` | aapcs64と同じ | 同左 | 同左 | 同左 | x18 は予約(使用禁止)。x29 は有効なframe recordを指す |
| `win-arm64` | aapcs64と同じ | 同左 | 同左 | 同左 | x18 は予約(TEB) |

**注意:** この表は本書作成時に一次資料を開いて照合していない `[U]`。実装時に AAPCS64・Apple・Microsoft・System V psABI の各資料で確認し、資料の版を ABI データに記録する。

- 初期の入出力は64bit以下の整数とポインタに限る。可変長引数・構造体値・浮動小数点の引数は `UNSUPPORTED_FEATURE`。
- SIMD/FPレジスタの保存を検査しない構成では、その claim を NOT_EVALUATED にし、「ABI全体をPASS」としない。
- **red zone の扱い:** `sysv-x86_64` では入口RSPの下128byteへの書込みはスタック領域内の正当な書込みとする。`win64` ではそうしない。スタック領域の許可範囲はABIから計算する。
- 「復帰時に元通り」と「使用禁止」は別の claim(`callee_saved` と `reserved`)。

## 5.5 Executor の実装

### emulated(Unicorn)

- 第一候補は Unicorn。x86_64 と aarch64 を同じエンジンで扱える。採用条件は、各ホストでの適格試験の合格とライセンス確認(10章)。
- フック: 命令(PCがcode領域内か、sentinel到達)、メモリアクセス(幅を含む半開区間 `[addr, addr+width)` が許可領域に完全に収まるか)、割込み・syscall命令(作用モデルへ)、不正命令・未割当アクセス。
- 未割当アドレスへのアクセスで自動的にページを割り当てない。未定義領域を0で補わない。
- 命令単位・アクセス単位のフックは遅い `[R]`(一般にエミュレータのフックは翻訳済みブロックの高速実行を妨げるため。性能は未測定)。必須の監視と、詳細traceの保存を分けて設定する。

### native-routine(ホストISA = 対象ISA)

対象コードを子プロセスで実CPUに実行させる。Mukoz本体には読み込まない。

1. 子プロセスでメモリを確保し、コードをコピーしてから実行可・書込み不可にする(W^X)。
2. 小さなトランポリンがレジスタを設定し、入口へ分岐する。復帰後に全レジスタを保存する。
3. 不正アクセス・不正命令はシグナル(Windowsでは例外)で捕捉し、`crashed` として記録する。
4. 時間上限で子プロセスを終わらせる。

| 能力 | 可否 |
|---|---|
| 戻り値・保存レジスタ・SP | 可 |
| クラッシュの検出 | 可 |
| 領域外アクセス | guard ページで検出できる範囲だけ(ページ単位)。byte単位は不可 |
| 禁止作用(syscall)の試行 | 初期は不可(Linuxでは seccomp で後から追加できる見込み `[R]`) |

`emulated` との差分試験の相手として使う(06章 6.8)。

### native-process / translated-process

- shellを使わず、固定した実行ファイルと構造化した argv で起動する。環境変数は既定で空にし、Bindingの指定だけを渡す。
- stdout・stderrを同時に読み、出力上限・パイプ詰まり・子孫プロセスがfdを持ち続けることによる待ちを監督する。
- 観測を保証するのは、直接起動したプロセスの stdout/stderr のバイト列、終了状態またはシグナル、経過時間、起動失敗の分類まで。
- ファイルの前後比較が同じでも、一時的な書込みや外部送信は否定できない。

| OS | 起動 | 子孫の停止 | 実行するファイルの固定 |
|---|---|---|---|
| Linux | `posix_spawn` / `fork+exec`、新しいプロセスグループ | cgroup v2 の子グループを kill(委譲されている場合)。なければプロセスグループのみ | スナップショットを memfd にコピーし `execveat`/`fexecve` で起動。パスを再解決しない `[R]` |
| macOS | `posix_spawn`、プロセスグループ | プロセスグループのみ(離脱した子孫は止められない場合がある) | 専用ディレクトリへコピーし権限を絞る。パス再解決の余地が残ることを記録 |
| Windows | `CreateProcess`(引数は構造化して組み立てる) | Job Object(kill-on-close) | 専用ディレクトリへコピー |

- OSの実行ポリシーで止められた場合(macOS Gatekeeper、WindowsのSmartScreen / Mark-of-the-Web、Linuxの `noexec` マウント等)は `EXECUTION_BLOCKED_PLATFORM_POLICY`。機能の反例にしない。
- Mukozは quarantine 属性の解除、署名の追加、OSのセキュリティ設定の変更をしない。所有者が外で準備し、変更後のファイルを登録する。
- `translated-process` は変換層(qemu-user、Rosetta 2 等)の名前と版を `execution_platform` に記録する。現ホストには qemu-user がないため、初期は能力なしと報告される。

## 5.6 作用モデル

作用の**意味**と、OS×ISAごとの **syscall adapter** を分ける。

```text
trap(syscall命令)
   │
   ▼
syscall adapter(OS×ISA): 番号・引数レジスタ・戻り値・エラー表現の変換
   │
   ▼
作用の意味(OS非依存): write(fd, bytes) / exit(code) / …
   │
   ├─ 許可された作用 → 環境モデルが応答を決め、作用イベントを記録
   └─ それ以外       → 禁止作用の試行として停止(または未対応として停止)
```

ゲストのsyscallをホストOSへ転送しない。ホストのstdoutへ表示することと、ゲストのstdoutへの書込みをモデル化することは別である。

### adapter 表(初期)

| adapter | trap命令 | 番号 | 引数 | 戻り値・エラー | 番号の例 | 確認状況 |
|---|---|---|---|---|---|---|
| `linux-x86_64` | `syscall` | rax | rdi, rsi, rdx, r10, r8, r9 | rax、エラーは `-errno` | write=1, exit=60, exit_group=231 | write=1・exit=60 は手書きhelloの実行で1回観測。他は `[U]` |
| `linux-aarch64` | `svc #0` | x8 | x0–x5 | x0、エラーは `-errno` | write=64, exit=93, exit_group=94 | `[U]` |
| `darwin-aarch64` | `svc #0x80` | x16 | x0–x5 | x0、エラー時はキャリーフラグを立てx0にerrno | write=4, exit=1 | 番号は0.4がXNUの`syscalls.master`から引用。他は `[U]` |
| `darwin-x86_64` | `syscall` | rax(クラス接頭辞 `0x2000000` 付き) | rdi, rsi, rdx, r10, r8, r9 | rax、エラー時はキャリーフラグ | write=4, exit=1 | `[U]` |
| Windows | — | — | — | — | — | syscall番号は安定したABIとして公開されていない `[R]`。API(kernel32 / ntdll)単位のstubモデルが必要で、初期範囲外 |

実装時に各OSの一次資料(Linux カーネルの syscall table、XNU の `syscalls.master`)で番号を固定し、資料の版を adapter に記録する。

### 環境モデル

| モデル | 内容 |
|---|---|
| `linux-stdio/1` | write・exit・exit_group のみ。プロセス開始時のスタック(argc・argv・envp・最小限のauxv)をモデルの版として固定して作る |
| `darwin-stdio/1` | write・exit のみ。`LC_MAIN` の入口を `main(argc, argv, envp, apple)` として呼び、sentinel への復帰を `exit(x0)` とみなす。これはdyld・libSystemの振る舞いの**仮定**であり、証跡に明記する |
| `full-success/1` | すべての write が要求長だけ成功する |
| `scripted-write-results/1`(後段) | 短いwrite・失敗をスクリプトで注入する |

- 作用モデルは fd、バッファ、要求長、読み取った範囲、受理したbyte数、戻り値、イベント順序を記録する。要求長を「書けた長さ」とみなさない。
- 未実装の失敗応答を成功応答で代用しない。
- glibc 等を静的リンクした実行ファイルは、起動時に多くのsyscall(brk、arch_prctl等)を呼ぶ `[R]`。初期モデルでは `UNSUPPORTED_DURING_RUN` になる。初期の process fixture は libc を使わない手書きアセンブリにする。
- `darwin-stdio/1` の結果は、dyldの初期化・初期化関数・ライブラリロードを検査したことにならない。

## 5.7 エラー時の扱い

| 事象 | 扱い |
|---|---|
| 未割当アドレスへのアクセス | ページを自動割当しない。契約・Bindingで禁止なら反例、モデル外なら検査不能 |
| 未対応命令 | `UNSUPPORTED_DURING_RUN`。NOPに置き換えない |
| 未対応syscall | 契約で禁止された作用なら反例、そうでなければ `UNSUPPORTED_DURING_RUN` |
| 自己書換え | 対応しない。W^X違反として、契約で禁止なら反例、そうでなければ検査不能 |

W^X、分岐先の制限などは「OS全体の正しさの規則」ではなく、そのExecutor・環境モデルでの検査条件として表示する。
