# 02 プラットフォームモデル

Mukozは、**Mukozが動くホスト**と**検査される成果物のプラットフォーム**を別々の軸で扱う。どちらも特定の環境(Apple Silicon Mac等)を前提にしない。最初に動かす環境は Linux x86-64 ホストとする(2.6)。

## 2.1 軸

| 軸 | 値の例 | 決めるもの |
|---|---|---|
| ISA | `x86_64`, `aarch64` | 命令意味、レジスタ集合、エンジンの選択 |
| 形式 | `raw`, `elf`, `macho`, `pe` | Inspectorとローダー |
| ABI | `sysv-x86_64`, `win64`, `aapcs64`, `apple-arm64`, `win-arm64` | 引数・戻り値・保存レジスタ・スタック規則 |
| OS | `none`, `linux`, `darwin`, `windows` | 作用モデル(syscall adapter)、ネイティブ実行の可否 |
| Executor | `emulated`, `native-routine`, `native-process`, `translated-process` | 実行方式と観測・強制能力 |
| Host | `linux-x86_64`, `linux-aarch64`, `macos-aarch64`, `macos-x86_64`, `windows-x86_64`, `windows-aarch64` | Executorの可否と、結果の記録先 |

**Target platform** は (ISA, 形式, ABI, OS) の組で、`x86_64/elf/sysv-x86_64/linux` のように書く。rawルーチンは `aarch64/raw/aapcs64/none` のように OS を `none` にする。

0.4の「profile」(`aarch64-routine/1` 等)は、この組と Executor・環境モデルから導く**表示用の名前**に格下げする。判定や能力照合は軸の値で行う。

## 2.2 組み合わせの制約

軸は独立だが、組み合わせには制約がある。制約表はデータとして持ち、コードに散らさない。

| 制約 | 例 |
|---|---|
| 形式とOS | `macho` は `darwin`、`pe` は `windows`、`elf` は主に `linux`。`raw` は任意 |
| ABIとISA | `sysv-x86_64` / `win64` は `x86_64`、`aapcs64` / `apple-arm64` / `win-arm64` は `aarch64` |
| ABIとOS | `apple-arm64` は `darwin`、`win64` / `win-arm64` は `windows` |

制約に反する指定は `PLATFORM_COMBINATION_INVALID` として計画作成時に拒否する。

## 2.3 Executor

| Executor | 条件 | 主な能力 | 主な限界 |
|---|---|---|---|
| `emulated` | 対象ISAのエンジンが、そのホストで適格確認済み(2.5) | レジスタ・全メモリアクセス・trapの観測と強制 | エンジンの命令意味に依存。OSは作用モデルの範囲だけ |
| `native-routine` | ホストCPUのISA = 対象ISA | 実CPUでの戻り値・保存レジスタ・クラッシュ検出 | メモリアクセスはページ単位の保護しかない。byte単位の監視不可 |
| `native-process` | ホストOS = 対象OS、かつホストCPUが対象ISAを実行できる | 実OSローダー・実stdout/stderr・終了状態 | 全syscall・全メモリ・通信の不在は観測できない |
| `translated-process` | ホストに対象ISA→ホストISAの変換層がある(qemu-user、Rosetta 2、WindowsのARM上x64エミュレーション) | `native-process` と同種 | 変換層の版・挙動に依存。実機の結果とは別に記録する |

- Executorの間で**自動fallbackしない**。エミュレーションで未対応だったからネイティブで動かす、ということをしない。
- 変換実行を `native-process` と同一視しない。証跡には変換層の名前と版を残す。

## 2.4 ホストごとの実行可否

`O` は可、`-` は不可、`T` は変換層があれば可。

| Host \ 対象 | x86_64 routine | aarch64 routine | x86_64/elf/linux | aarch64/elf/linux | aarch64/macho/darwin | x86_64/macho/darwin | x86_64/pe/windows |
|---|---|---|---|---|---|---|---|
| `linux-x86_64` | emu, native | emu | emu, native | emu, T | emu | emu | emu |
| `linux-aarch64` | emu | emu, native | emu, T | emu, native | emu | emu | emu |
| `macos-aarch64` | emu | emu, native | emu | emu | emu, native | emu, T(Rosetta) | emu |
| `macos-x86_64` | emu, native | emu | emu | emu | emu | emu, native | emu |
| `windows-x86_64` | emu, native | emu | emu | emu | emu | emu | emu, native |

- `emu` は「エンジンが対応していれば」の意味。実際に何が使えるかは、ホストでの能力確認(2.5)で決まる。
- `emu` 欄の process 対象は、そのOSの作用モデルが実装されている場合に限る。Windowsの作用モデルは初期範囲外(05章 5.6)。
- この表は設計上の想定であり、`linux-x86_64` 以外はどれも試していない。

## 2.5 能力の確認と照合

Executorが返す能力は、**宣言値ではなくホスト上で確認した値**とする。

1. `mukoz platform probe` がホスト情報(OS版、CPU、CPU機能)と、各Executorの能力を調べ、`HostProbe` として保存する。
2. `emulated` は、ISAごとの適格試験(既知の答えを持つ命令試験。09章)にそのホスト・そのエンジンビルドで通ったときだけ「使用可」にする。通った記録を `EngineQualification` とする。
3. 隔離能力(ネットワーク遮断、ファイルシステム制限、子孫プロセスの停止等)も、実際に試して効いたものだけを返す(08章)。

Plannerは、必須claimごとに必要な能力を計算し、選んだExecutorの確認済み能力と照合する。

```text
必要:  対象外メモリへの書込みをすべて検出する
選択:  native-routine(ページ単位の保護のみ)
結果:  REQUIRED_CAPABILITY_UNAVAILABLE → そのclaimは NOT_EVALUATED → HOLD
```

ホストが対象を実行できないこと(例: Linux上で Mach-O を native 実行)も同じ経路で扱い、理由を `HOST_CANNOT_EXECUTE_TARGET` とする。OSの実行ポリシーで止められた場合(`EXECUTION_BLOCKED_PLATFORM_POLICY`)とは区別する。

## 2.6 ホストの支援段階

| 段階 | 意味 |
|---|---|
| Tier 1 | 受入試験をそのホストで継続して回し、通っている |
| Tier 2 | ビルドとコア試験が通る。Executorの一部は未確認 |
| Tier 3 | 設計上は対応するが、ビルドしていない |

初期状態はすべて Tier 3。最初に `linux-x86_64` を Tier 1 にする(09章 P1〜P4)。次に `macos-aarch64`、その後 `linux-aarch64`、`windows-x86_64` の順を想定する。順序は未決定で、10章の未決事項に置く。

**現環境 `linux-x86_64` で最初に扱う対象:**

| 対象 | Executor | 段階 |
|---|---|---|
| `x86_64/raw/sysv-x86_64/none` ルーチン | `emulated`、`native-routine` | P1 |
| `aarch64/raw/aapcs64/none` ルーチン | `emulated` | P2 |
| `x86_64/elf/sysv-x86_64/linux` 静的・libcなし | `emulated`(Linux作用モデル)、`native-process` | P3 |
| `aarch64/macho/apple-arm64/darwin` hello | `emulated`(Darwin作用モデル)。native は `HOST_CANNOT_EXECUTE_TARGET` | P3 |

ホストに依存するコードは Executor と Host probe の中に閉じ込める。Core(契約・計画・判定・証跡)は、ホストのOS・CPU・エンディアンに依存しない。値はすべて明示したbyte順で読み書きし、ホストのbyte順に依存させない。ただし big-endian ホストは試験の対象に入れない(支援段階を付けない)。
