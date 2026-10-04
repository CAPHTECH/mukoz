# Mukoz 仕様・設計書

**版:** 0.5-design-draft.1(2026-10-04)
**状態:** 実装前の設計。ここに書いた機能・CLI・性能はどれも実装・実測していない。
**前版:** 0.4-design-draft.1。本書群に置き換えて削除した(経緯は [10](10-decisions.md) 10.4)。

**ゴール:** AIエージェントが、ソースなしで、契約を満たすバイナリを生成・修正できるようになること。Mukozの判定の信頼性はそのための制約であり、緩めない。最初の探索目標は、この価値の仮説の比較試験とする([11](11-development-process.md))。

Mukozは、実行成果物(バイナリ、または入口を明示した機械語ルーチン)が、書かれた契約をどの条件・範囲で満たしたかを調べ、反例と保証範囲を機械可読な証跡として返すテストツールである。

## 0.4からの主な変更

| 変更 | 理由 | 詳細 |
|---|---|---|
| 対応環境を「実行ホスト」と「検査対象プラットフォーム」の独立した軸で定義する | ARM Mac以外のホスト・対象を両方サポートする要件 | [02](02-platform-model.md) |
| 最初の動作環境を Linux x86-64 ホストにする | 現在の開発環境。ここで動くことを最初の完了条件にする | [02](02-platform-model.md)、[09](09-implementation-plan.md) |
| 証跡の同一性を「検査対象context」と「実行プラットフォーム」に分ける | ホストをcontextに混ぜると別ホストの証跡がすべてHOLDになり、混ぜないとホスト差が記録されない | [03](03-architecture.md) |
| 契約に「プロセス境界」を追加し、stdout・終了コードを契約で書けるようにする | 0.4はhelloの期待出力を書く手段が契約側になかった | [04](04-contract-language.md) |
| 式を手書きASTから、型付きASTへ変換する短いテキスト式に変える | `a+b` に20行要った。ASTは正本として残す | [04](04-contract-language.md) |
| 可変長メモリ範囲と有界量化子を式に加える | 0.4の `bytes(N)` 固定長では buffer copy を書けなかった | [04](04-contract-language.md) |
| 入力ファイルを YAML から TOML(と JSON)へ変える | YAMLの alias・tag・重複key対策が不要になる | [10](10-decisions.md) |
| OS作用モデルを「共通の作用意味」と「OS×ISAのsyscall adapter」に分解する | Darwin・Linux・Windowsで同じ write/exit の意味を使い回すため | [05](05-execution.md) |
| 同ISAホストでのネイティブ・ルーチン実行を追加する | 実CPUとエミュレータの比較を安く得られる | [05](05-execution.md) |
| Mukoz自身の実行ファイルと部品を Mukoz で検査し、判定器の独立性を claim に記録する | 回帰とプラットフォーム差を捕まえるため。自己検査だけでは正しさの根拠にしない | [06](06-evidence-and-judgement.md)、[09](09-implementation-plan.md) |
| AIの生成と検査の繰り返しを支える: 成果物を Suite/CLI で指定、回帰ケース、修正用の診断情報、`--fail-fast`、native実行の試行区域 | 0.5 初稿では成果物を指定する手段がなく、再発確認・修正の手がかりも弱かった | [04](04-contract-language.md)、[06](06-evidence-and-judgement.md)、[07](07-interface.md)、[08](08-security.md) |
| MVPのインターフェースを JSON出力のCLIに絞る。JSON-RPC・receipt・HMACは後段 | テストを1本も走らせる前に周辺機構を作らない | [07](07-interface.md) |
| FSL連携と形式検証(refinement)を本書群の範囲から外す | 指示による。拡張の余地だけ残す | [10](10-decisions.md) |

## 読む順序

1. [01 概念と保証の境界](01-concept.md)
2. [02 プラットフォームモデル](02-platform-model.md)
3. [03 アーキテクチャとデータモデル](03-architecture.md)
4. [04 契約・Binding・Suite](04-contract-language.md)
5. [05 成果物の検査と実行](05-execution.md)
6. [06 証跡と判定](06-evidence-and-judgement.md)
7. [07 インターフェース](07-interface.md)
8. [08 セキュリティ](08-security.md)
9. [09 実装計画と検証](09-implementation-plan.md)
10. [10 設計判断・未決事項・参考資料](10-decisions.md)
11. [11 開発の進め方](11-development-process.md) — ゴール、最初の探索目標、品質契約、工程
12. [12 エージェント向けの使い方](12-agent-guide.md) — 生成・修正のループ、REJECT / HOLD の読み方、契約・Suite の書き方
13. [13 プロセス境界とモジュール分割(実装)](13-process-and-modules.md) — システムコールの作用モデル、ファイル、リンクファイル、境界監視と責任の所在

## 表記

- 「する」「しない」「必須」は新実装の要件を表す。
- `[U]` は未確認。`[R]` は推論で、根拠と外れる条件を併記する。
- 「観測」は本書作成時に実際に行った確認を指し、回数を書く。

## 本書作成時に確かめたこと・確かめていないこと

確かめたこと(2026-10-04、作業ホスト `linux-x86_64`、Debian 13):

- `uname` は x86_64、カーネル 6.12。rustc / cargo 1.97.1、cmake 3.31、gcc 14.2、GNU binutils 2.44(x86-64 用 `as` / `ld` / `objdump`)がある。
- Unicorn・Capstone のシステムライブラリは pkg-config で見つからない。AArch64 を逆アセンブルできる objdump はない。qemu-user はない。
- cgroup v2 がマウントされている。LSM に landlock がある。`unshare -Urn true` は1回成功した。
- `48 8d 04 37 c3` は objdump で `lea rax,[rdi+rsi*1]; ret`、`48 89 f8 48 29 f0 c3` は `mov rax,rdi; sub rax,rsi; ret` と1回デコードされた。
- 手書きアセンブリの静的 x86-64 ELF hello を as/ld で作り、1回実行して stdout `Hello from raw x86-64 ELF!\n`(27 bytes)、終了コード 0 を得た。

確かめていないこと:

- Unicorn / Capstone の Rust binding がこのホストでビルドできるか `[U]`。
- AArch64 の機械語列(`8b010000` 等)のデコード `[U]`。0.4の記述を引き継いだ。
- 0.4 が参照した旧Mukoz v0.3 の実装・schema。このディレクトリに存在しない。
- macOS・Windows・Linux arm64 ホストでの動作。すべて設計のみ。
- 0.4 の `hello-arm64` Mach-O fixture。このディレクトリに存在しない。
