# 10 設計判断・未決事項・参考資料

## 10.1 設計判断

| ID | 判断 | 理由 | 0.4との関係 |
|---|---|---|---|
| ADR-01 | 正解は Contract から決める | 生成者の説明や対象の出力から期待値を逆算すると、誤りが共通化する | 継承(D01) |
| ADR-02 | Contract(ISA非依存)と Binding(プラットフォーム依存)を分ける | 同じ契約を複数ISA・複数ホストで使い回せる | 継承(D02)。複数ISAの根拠として重要度を上げた |
| ADR-03 | プラットフォームを ISA・形式・ABI・OS・Executor・Host の独立した軸で表す。profile名は表示用 | ARM Mac以外のホスト・対象を両方サポートする要件 | 変更(0.4のD08「初期はAArch64とMach-Oに限定」を置き換え) |
| ADR-04 | 同一性を subject context と execution platform に分ける。後者はclaimの範囲 | ホストを照合キーにすると別ホストの結果がすべて無効になり、入れないとホスト差が残らない | 変更 |
| ADR-05 | Executor の間で自動fallbackしない。能力はホストで確認した値だけを使う | 宣言値・fallbackは保証範囲を黙って変える | 継承(D09)を一般化 |
| ADR-06 | 最初の動作環境は Linux x86-64 | 現在の開発環境。ここで検証できない設計は進められない | 変更(0.4は Apple Silicon 実機を前提にしていた) |
| ADR-07 | 実装言語は Rust | 信頼できない入力を解析するためのメモリ安全性。`no_std`・`extern "C"` でランタイムに依存しない関数を作れ、自己検査の対象にできる。複数OS・複数ISAへのビルド | 継承。理由を追加 |
| ADR-08 | 人が書く形式は TOML、正本は JSON。式は文字列で書き、型付きASTに変換して保存 | YAMLの alias・tag・暗黙の型変換への対策が要らない。式を短く書ける | 変更(0.4はYAML+JSON AST) |
| ADR-09 | 式に可変長 bytes と有界量化子を入れる | バッファを扱うルーチンの契約を書けるようにする | 追加 |
| ADR-10 | プロセス境界の契約(stdout・stderr・exit)を入れる | helloのような実行ファイルの期待を契約で書けるようにする | 追加 |
| ADR-11 | 作用モデルを「作用の意味」と「OS×ISAの syscall adapter」に分ける | Linux・Darwinで意味を共有し、Windowsは別の adapter(API stub)で足せる | 変更 |
| ADR-12 | 同ISAホストでの native-routine を加える | 実CPUという独立した期待値の出どころを安く得られる | 追加 |
| ADR-13 | MVPのインターフェースは JSON出力のCLIのみ。JSON-RPC・MCP・receipt・HMAC は後段 | テストを走らせる前に周辺機構を作らない | 変更(0.4は JSON-RPC と12操作を継承) |
| ADR-14 | 自己検査を行い、claim に判定器の独立性を記録する。`self` だけでは既定で HOLD | 同じ誤りを持つ判定器はその誤りを見逃す | 追加 |
| ADR-15 | `release_authorized` は常に false | テストの合否とリリースの許可は別の判断 | 継承(D10) |
| ADR-16 | FSL連携と形式検証(有界検証・refinement)を本書群の範囲から外す。データモデルに `method` と評価値の予約だけ残す | 指示による。まず有限テストを確実にする | 変更 |
| ADR-17 | 検査する成果物は Suite の `[artifact]` か CLI の `--artifact` で指定し、Binding には書かない。raw の code 領域の既定はファイル全体 | AIが生成のたびにファイルや命令長を変えても、契約・Bindingを書き直さずに繰り返せるようにする | 変更(0.4は Binding の `target_id`) |
| ADR-18 | 反例を契約digest・ターゲットに結び付けた回帰ケースとしてため、次の検査で先に実行する。自動では消さない | 生成の繰り返しで再発を毎回確かめるため | 追加 |
| ADR-19 | finding に診断情報(失敗地点、直近の実行命令、違反アクセス、部分式の値)を付ける。判定には使わない | AIが機械語を直すための手がかり。診断と判定を混ぜない | 追加 |
| ADR-20 | `--fail-fast` で最初の反例後に打ち切れる。REJECT は有効、ACCEPT にはならない | 繰り返しの速度 | 追加 |
| ADR-21 | native実行は、digest指定の許可に加え、所有者が設定した試行区域で、要求する隔離能力がすべて確認できた場合に許す。既定は無効 | 生成のたびの人の許可で繰り返しが止まらないようにする。安全性との取引なので所有者が選ぶ | 追加 |

## 10.2 未決事項

| 論点 | 現在の案 | 決めるのに必要なもの |
|---|---|---|
| Unicorn のビルドと動作 | 第一候補 | P1-0 の spike。失敗した場合の代替: 自前の小さな命令インタープリタ(対応命令を絞る)、または別のエミュレータ |
| Mukoz のライセンス | 未定 | Unicorn は上流が GPLv2 を掲げている(0.4の確認。本書作成時は未再確認 `[U]`)。worker を別プロセスにしてもライセンス上の問題が解消するとは限らない。配布形態とあわせて決める |
| 次に Tier 1 にするホスト | macOS arm64 → Linux arm64 → Windows x86_64 | 利用者・CI環境の有無 |
| aarch64 fixture の作り方 | `binutils-aarch64-linux-gnu` か Rust の aarch64 ターゲット | パッケージ導入の可否 |
| Mach-O hello fixture | 0.4 の `hello-arm64` を取り寄せる、または手で組み立てる | ファイルの所在(このディレクトリにはない) |
| cgroup の委譲 | 使えれば子孫停止・メモリ制限に使う | 現ホストでの確認 `[U]` |
| Windows の作用モデル | API(kernel32 / ntdll)単位の stub | 契約付き stub の設計、Windowsホスト |
| ABI表・syscall番号の一次資料での照合 | 05章の表は未照合 `[U]` | 実装時に各資料を開いて版を固定 |
| 旧Mukoz v0.3 との互換 | 互換を前提にしない | 旧実装・schemaの所在 |
| JSON-RPC / MCP | 後段 | CLIの操作が固まった後 |
| 契約・Suite の改変防止 | **当面は設けない**(2026-10-04 指示)。AIが ensures を緩めたり Suite のケースを減らしたりすると、ACCEPT になり得る。現状は、証跡に契約・Suite の digest が残るので後から気付ける、という程度 | 必要になったら: 所有者が承認した Contract・Suite の digest を policy に固定し、違えば HOLD にする(0.4の承認済み契約に相当) |

これらは P0(型・式・判定・Store)の着手を止めない。

## 10.3 採用しない近道

- 生成者と同じロジックで期待値を作る
- 逆アセンブルできたことを成功とする
- 未対応のsyscallに成功応答を返す、未対応命令をNOPにする
- 最初の `ret` で終了する
- 初期レジスタをすべて0にして、それをABIの保証とみなす
- native実行で観測していない作用を「なかった」とする
- 時間切れを成功とする
- エミュレーションで動かないものを黙ってnative実行する
- 署名前後を同一視する、`.text` の一致だけで証跡を再利用する
- 別ホストで得た結果を、ホストを記録せずにまとめる
- 自己検査の結果を独立した検査と同列に扱う
- テストの報告を証明書と呼ぶ

## 10.4 参考資料

本書作成時(2026-10-04)に**開いて再確認していない**。0.4 が参照した資料と、本書で新たに挙げた資料の一覧であり、実装時に版を固定して確認する。

| ID | 資料 | 用途 |
|---|---|---|
| R1 | Apple, *Writing ARM64 code for Apple platforms* — <https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms> | `apple-arm64` ABI |
| R2 | Arm, *Procedure Call Standard for the Arm 64-bit Architecture (AAPCS64)* — <https://github.com/ARM-software/abi-aa/blob/main/aapcs64/aapcs64.rst> | `aapcs64` ABI |
| R3 | *System V Application Binary Interface, AMD64 Architecture Processor Supplement* | `sysv-x86_64` ABI |
| R4 | Microsoft, *x64 calling convention* / *ARM64 ABI conventions*(Microsoft Learn) | `win64`、`win-arm64` ABI |
| R5 | Unicorn — <https://github.com/unicorn-engine/unicorn> | エミュレーション、フック、ライセンス |
| R6 | Capstone — <https://www.capstone-engine.org/> | 逆アセンブル |
| R7 | `object` crate — <https://docs.rs/object/latest/object/> | 形式の読取り |
| R8 | Apple XNU `EXTERNAL_HEADERS/mach-o/loader.h` — <https://github.com/apple-oss-distributions/xnu> | Mach-O 構造 |
| R9 | Apple XNU `bsd/kern/syscalls.master` — 同上 | Darwin syscall 番号 |
| R10 | Linux カーネル `arch/x86/entry/syscalls/syscall_64.tbl`、`include/uapi/asm-generic/unistd.h` | Linux syscall 番号 |
| R11 | *ELF-64 Object File Format* / System V gABI | ELF 構造 |
| R12 | Microsoft, *PE Format* | PE 構造 |
| R13 | Apple, *TN2206: macOS Code Signing In Depth* | 署名と変更 |

前版 0.4-design-draft.1(`Mukoz_Binary_Test_Tool_Design.md`)は、本書群に内容を移したうえで削除した(2026-10-04)。FSL連携・形式検証・旧Mukoz v0.3 からの移行表は、移さずに捨てた。

0.4 が参照していた旧Mukoz v0.3 の内部資料(このディレクトリにはない): `DESIGN(6).md`(v0.3.0 設計)、`PROTOCOL.md`(`mukoz/1` 通信仕様)、`NAMING(1).md`(名称規約)、`README(20261001-202535).md`(試作状態。旧Rust実装の build/test は未確認と記載)。
