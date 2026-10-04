# 09 実装計画と検証

## 9.1 言語と方針

- 実装言語は **Rust**。理由は10章 ADR-07。
- 小さなコアから始める。大きなマイクロサービス群や動的プラグインABIにしない。
- ホストに依存するコードは `mukoz-platform` と worker の中に閉じ込める。Core はホストのOS・CPUに依存しない。
- 最初の完了目標は **現環境 `linux-x86_64`(Debian 13)で P1〜P4 が通ること**。

## 9.2 リポジトリ構成(案)

```text
mukoz/
  Cargo.toml                  # workspace
  crates/
    mukoz-kernels/            # #![no_std]。bv演算・範囲検査など純粋な処理。Core と自己検査の両方が使う
    mukoz-core/               # 型、式(構文解析・型検査・評価)、Contract/Binding/Suite、計画、採否
    mukoz-artifact/           # スナップショット、raw/ELF/Mach-O/PE の検査、LoadPlan
    mukoz-platform/           # ABI表・syscall adapter表(データ)、HostProbe、能力モデル
    mukoz-store/              # content-addressed store、索引、射影
    mukoz-proto/              # worker IPC(版付き)
    mukoz-worker-emu/         # bin。Unicorn(FFIはここだけ)
    mukoz-worker-native/      # bin。native-routine / native-process(OS別に cfg)
    mukoz-cli/                # bin `mukoz`
  fixtures/
    <name>/{src.s, <isa>.bin, manifest.toml}   # 元のアセンブリ、生成物、digest・ツールチェーン
  conformance/
    x86_64/ aarch64/          # エンジン適格試験(既知の答えを持つ命令試験)
  selfcheck/
    contracts/ suites/ checkers.toml          # 自己検査用(9.6)
  schemas/
  tests/
  docs/
```

## 9.3 依存候補

| 領域 | 候補 | 確認事項 |
|---|---|---|
| 形式の読取り | `object` | ELF/Mach-O/PE の読取りAPI。整合検査は自前 |
| CPUエミュレーション | Unicorn 2 + Rust binding(`unicorn-engine`) | このホストでのビルド `[U]`(システムライブラリはない。同梱ソースをcmakeでビルドする方式を想定)、ライセンス(10章) |
| 逆アセンブル | Capstone + `capstone` crate | 表示用。必須経路に入れない |
| 構造化I/O | `serde`、`serde_json`、`toml` | 未知フィールド拒否、重複キー拒否、上限付きの読込み |
| digest | `sha2` | 正規化JSONに対して計算 |
| 乱数 | **自前の小さなPRNG**(例: SplitMix64) | 外部crateの版が変わると生成列が変わり得るため `[R]`。生成器の版を証跡に記録する |
| Linuxのプロセス制御 | `rustix` 等 | memfd、execveat、cgroup、namespace |
| Windows | `windows-sys` | 後段 |

採用時に版(または commit)を固定し、`latest` に追従しない。

## 9.4 段階

**実行順序は 11章 11.5 に従う。** 下の P0〜P5 は作る機能のまとまりを示す。11章では P1 と P3 の一部(領域・ポインタ)を先に作り、`linux-x86_64` の Tier 1 受入(9.8)は比較試験(11.2)の後に置く。

| 段階 | 作るもの | 完了条件(すべて `linux-x86_64` 上) |
|---|---|---|
| **P0 Core** | kernels、式言語、TOML/JSON読込み、Store、Assessor、プラットフォーム表、`platform probe`(ホスト情報のみ) | 式評価の既知値試験、未知フィールド拒否、STALE/UNKNOWN→HOLD、VACUOUS_SCOPE の試験が通る |
| **P1-0 Spike** | Unicorn の Rust binding をビルドし、x86_64 の add64 を1件実行する捨てコード | ビルドと1件の実行ができる。できなければ10章の代替案を検討してから先へ進む |
| **P1 x86_64 ルーチン** | emulated worker、x86_64 SysV ハーネス、sentinel、メモリ監視、計画・生成、`check`(`--artifact`・`--fail-fast`)、回帰ケース、診断情報(PCリングバッファ・値の食い違い・違反アクセス)、x86_64 適格試験、native-routine worker、差分試験 | 9.5 の最初の縦切りが通る |
| **P2 aarch64 ルーチン** | aarch64 ハーネス(AAPCS64)、aarch64 適格試験 | add64 / sub変異 / nested call を**同じContract**で検査できる |
| **P3 プロセス** | ELF Inspector、`linux-stdio/1`(x86_64・aarch64 adapter)、Linux native-process、Linuxの隔離能力 probe と試行区域、Mach-O Inspector、`darwin-stdio/1` | x86_64 ELF hello が emulated と native-process の両方で ACCEPT、変異が REJECT。Mach-O hello が emulated で検査でき、native は `HOST_CANNOT_EXECUTE_TARGET` で HOLD |
| **P4 自己検査と仕上げ** | `replay`・`shrink`・`show` のページと `--disasm`(Capstone)、`regressions` コマンド、自己検査の段階1〜3(9.6) | 9.8 の受入基準を満たす → `linux-x86_64` を Tier 1 にする |
| P5 他ホスト | macOS arm64 → Linux arm64 → Windows x86_64(PE、win64、API stubモデル) | ホストごとに Tier 2 → Tier 1 |
| 後段 | JSON-RPC、故障注入スクリプト、有界検証 | 10章 |

P1-0 を最初に置くのは、Unicorn のビルドと動作がこの計画で最大の不確実性だからである。

## 9.5 最初の縦切り

```text
fixtures/add64 (x86_64: 48 8d 04 37 c3)
  + arith.add64 Contract
  + arith.add64@x86_64-sysv Binding
  + 境界値直積 + random 4096
        ↓
mukoz check
        ↓
正しい実装:        ACCEPT_WITHIN_SCOPE
sub 変異:          REJECT + arith.add64/sum + 反例 + 診断情報
                   → 反例が回帰ケースとして保存される
sub 変異を再検査:  --fail-fast で回帰ケース reg-… が最初に失敗する
正しい実装に戻す:  回帰ケースを含めて ACCEPT_WITHIN_SCOPE
未対応命令を含む:  HOLD + UNSUPPORTED_DURING_RUN
native-routine 可: 同じケースで emulated と一致
        ↓
mukoz show / mukoz replay
```

この段階では ELF・Mach-O・プロセス・自己検査を同時に入れない。

## 9.6 自己検査

Mukozの実行ファイル(Linuxでは x86_64 ELF)と、その部品を、Mukoz 自身で検査する。目的は**回帰とプラットフォーム差を捕まえる網**を持つことで、Mukozの正しさの証明ではない。

### 段階

| 段階 | 検査するもの | Executor | 時期 |
|---|---|---|---|
| 1 | 答えが分かっている fixture 群・変異 fixture に対する Mukoz の判定(9.7) | 全部 | P0から |
| 2 | `mukoz` CLI をプロセス境界の契約で検査(入力ファイルに対する stdout の JSON・終了コード) | native-process | P3以降 |
| 3 | `mukoz-kernels` の関数を `extern "C"` の入口でルーチンとして検査。x86_64 と aarch64 の両方へビルドする | emulated、native-routine | P4 |
| 4 | Rust標準ライブラリを使うプロセス全体のエミュレーション | — | 当面しない。Linux syscall の広いモデルが必要になるため |

### 段階3の対象と期待値の出どころ

循環を避けるため、層ごとに期待値の出どころを決める。

| 層 | 例 | 期待値の出どころ |
|---|---|---|
| 最下層: bv演算 | `mk_bv_add64`、比較、シフト、符号拡張 | 実CPUの命令結果(native-routine で同じ演算命令を実行)と、手で確定した値の表。Mukozの式評価器は使わない |
| 中間層: 範囲検査 | `mk_range_contains(base, size, addr, width)`(メモリ監視の中核) | 式言語で書いた契約。式評価器は最下層の検査済み演算に依存する |
| 上位層: 構造検査 | ELF header の検査関数(バッファを受け取る) | 手で作った正常・異常ヘッダの表 |

- kernels は `#![no_std]`、`panic = "abort"`、メモリ確保なし、`#[no_mangle] extern "C"` で作る。
- コンパイラが `memcpy`・`memset` 等の呼出しを挿入した場合、その関数は外部呼出しを含むので、ルーチン検査では `UNRESOLVED_DEPENDENCY` で HOLD になる。これを正しく検出することも試験項目にする。
- 同じソースから作った判定器と対象を比べて見つかるのは、主にコンパイラ・最適化レベル・ISAによる差である。ロジックの誤りは、上の期待値の出どころで捕まえる。

### 判定器の版

- `selfcheck/checkers.toml` に、検査に使う前の版の `mukoz` 実行ファイルの digest を記録する。版 N の自己検査は、版 N-1 の判定器で行った結果を必須とし、版 N 自身での結果は `independence = self` として追加で記録する(06章 6.9)。
- 最初の版(N-1 がない)は、段階1の既知答え試験と、最下層の実CPU照合だけを根拠にする。

## 9.7 Mukoz自身の試験

### fixture

| fixture | ISA | 確認すること |
|---|---|---|
| add64 / sub64 | x86_64, aarch64 | 幅、算術、結果のBinding |
| signed比較 / unsigned比較 | 両方 | 比較の符号を混同しない |
| checked increment | 両方 | 入口ポインタの固定、状態更新、失敗系、frame |
| bounded copy | 両方 | 領域、アクセス幅、境界、alias |
| nested call | 両方 | 最初の `ret` で止まらない |
| scratch stack / red zone | x86_64 | 正当な一時書込みを禁止作用と誤認しない |
| callee-saved 破壊 | 両方 | ABI claim |
| x18 一時使用 | aarch64 | `apple-arm64` では違反。`aapcs64`(Linux)での扱いは ABI 表の確認後に決める `[U]` |
| hello ELF(libcなし) | x86_64 | 入口解決、仮想stdout、exit、native との一致 |
| 等価な hello | x86_64 | 違う命令列・同じ出力を受け入れる |
| hello Mach-O | aarch64 | `LC_MAIN`、`darwin-stdio/1` |
| 壊れた ELF / Mach-O | — | header・command・segment の境界、整数overflow |
| 動的リンクの ELF | x86_64 | `UNRESOLVED_DEPENDENCY` を機能違反にしない |
| 未対応命令 | 両方 | NOP化・fallbackをしない |
| 無限ループ | 両方 | 予算切れと停止性違反を区別 |

各 fixture は、元のアセンブリ、生成物、digest、使ったツールチェーンの版を `manifest.toml` に持つ。

**fixture作成(2026-10-05 の状態):** aarch64 は Rust の `aarch64-unknown-linux-gnu` ターゲット(`global_asm`)と `llvm-objcopy` で作る(`fixtures/aarch64/build.py`)。Mach-O は手で組み立てる(`fixtures/process/mkmacho.py`)。下は本書作成時の記録。

**fixture作成の制約(本書作成時):**

- x86_64 は `as` / `ld` で作れる(helloは1回作って実行できた)。
- aarch64 のアセンブラ・逆アセンブラがない。`binutils-aarch64-linux-gnu` の導入(apt、要確認)か、Rust の `aarch64-unknown-linux-gnu` ターゲットの追加が必要。どちらもしていない。
- Mach-O の hello を作る Apple のツールチェーンはない。0.4 の `hello-arm64` を取り寄せるか、手で組み立てる必要がある(10章 未決事項)。
- 0.4 に記録されていた `hello-arm64` の値(本書作成時には再確認していない。ファイルもこのディレクトリにない):

  | 項目 | 署名前(0.4作成時に確認) | 署名後(ユーザー報告のみ) |
  |---|---|---|
  | サイズ | 16,384 bytes | — |
  | SHA-256 | `a10c4e12b5e8d1f62b649ed16d2384e04db7081b03b6e18dea2ce547d1c418cc` | `fe607f74d161462411391222959671488698f5b09952e9570f6ddbf8bac71e67` |
  | header | ncmds = 6、sizeofcmds = 376 | ncmds = 7、sizeofcmds = 392 |
  | 命令 | file offset `0x300`、32 bytes(8命令。writeの戻り値を確認しない) | — |
  | メッセージ | file offset `0x320`、29 bytes(`Hello from raw ARM64 Mach-O!` + 改行と推定 `[R]`) | — |

  ARM Mac 上で起動して文字列が表示されたことは報告されているが、stdout のバイト列・終了状態を自動で取得した試験ではない。

### 変異

正常な fixture から意味の分かる変異を作り、**どの契約のどの性質に違反するか**を先に決める。

```text
add → sub、64bit → 32bit、signed分岐 → unsigned分岐
許可範囲外へ1byte書く、対象外へ書いてから戻す
callee-saved を壊す、予約レジスタを一時利用して戻す
不正な復帰先へ戻る、stdout の長さを1byte増減
write の失敗を無視する(full-success環境では反例にならない。故障注入付きの別suiteで調べる)
入口や load command を壊す、不要な外部作用を試みる
```

### 判定器の負の試験

| 対象 | 試験 |
|---|---|
| Artifact | inspect後の変更、署名による変更、同名の別ファイル |
| Binding | 入口のずれ、結果レジスタのずれ、幅の誤り、古いdigest |
| Plan | ケース0件、全ケースが事前条件外、必須claimの迂回、予算不足 |
| Assessor | UNKNOWN→PASS への誤変換、後続障害による反例の消失、古い REJECT |
| Platform | 適格記録のないホストの emulated 結果、HOST_CANNOT_EXECUTE_TARGET、Executor間の自動fallback |
| Evidence | trace欠落、射影の省略、replay対象の変更、書込み途中のクラッシュ |
| Engine | 前ケースからのレジスタ・メモリ・作用・キャッシュの漏れ |
| CLI | 未知フィールド、過大入力、`--gate` の有無による終了コード、`--artifact` も `[artifact]` もない場合 |
| 回帰ケース | 契約変更で古い集合が「当てはまらない」になる、Binding変更で当てはめられないケースが NOT_EVALUATED になる、上限超過で計画を拒否する、`include = false` が limitations に出る |
| fail-fast | 反例後に打ち切った run が ACCEPT にならない、未実行数が出る |
| 試行区域 | 隔離能力が1つ欠けたら `NATIVE_NOT_PERMITTED`、emulated へ切り替えない、シンボリックリンクで区域外を指すファイルを拒否する |
| 診断情報 | Capstone がなくても採否が変わらない、診断の上限超過で判定が変わらない |
| Security | 対象の出力による偽PASS、パス操作、ログ増幅 |

## 9.8 受入基準(`linux-x86_64` を Tier 1 にする条件)

これは将来満たす基準であり、現在の測定値ではない。

1. 9.7 の正常 fixture が、対応する Executor とケース集合で ACCEPT_WITHIN_SCOPE になる。
2. 対応範囲内の変異を、予告した性質IDで REJECT し、反例を replay できる。
3. ファイル・署名・契約の変更で、古い採否が流用されない。
4. 未対応・0件・時間切れ・欠測を成功にする経路がない(負の試験で確認)。
5. 同じ subject context・同じ execution platform でのケース再実行が同じ結果になる。異なれば理由を記録する。
6. native-process の結果に、観測していないメモリ・通信の保証を付けない。
7. AIエージェントが要約から finding・反例・必要なtraceへ段階的にたどれる。
8. x86_64 で emulated と native-routine の差分試験が、適格試験の範囲で一致する。
9. 自己検査の段階1〜3が通り、`independence` が正しく記録される。
10. **生成ループの試験:** AIエージェントが、Mukozの CLI 出力だけを手がかりに(Mukozの内部ファイルや fixture の正解を読まずに)、変異 fixture を直して REJECT から ACCEPT_WITHIN_SCOPE まで到達できる。到達までの反復回数と、途中で回帰ケースが再発を捕まえた回数を記録する。

「誤った受理ゼロ」は fixture 群の中での目標であり、未知のバイナリ全般についての保証ではない。

**達成状況(2026-10-05、作業ホスト `linux-x86_64`、各1回):** `tools/tier1.py` が基準ごとに決める試験と run を回し、`target/tier1-report.json` に書く。

| 基準 | 根拠 | 結果 |
|---|---|---|
| 1・2 | `tests/acceptance.rs`・`tests/coverage.rs`(両 ISA の fixture 表)・`tests/native.rs`・`tests/macho.rs`、shrink → replay | 通過 |
| 3 | `tests/negative.rs`: artifact・同名の別ファイル・契約・Binding・Suite の変更で subject context が変わり、古い回帰集合は「当てはまらない」と数える | 通過 |
| 4 | I1 系の試験、0件・時間切れ・停止しない native・壊れた証跡・上限超過・Capstone なしのビルドで採否が同じ | 通過 |
| 5 | 同じ subject の再実行で採否・claim・反例の入力と観測が同じ(違うのは run / 反例の ID だけ)、反例の単独 replay が一括実行と同じ観測、回帰ケースの ID と seed が実行をまたいで同じ | 通過 |
| 6 | native-process だけの run で memory・effects が NOT_EVALUATED | 通過 |
| 7 | `tests/navigation.rs` | 通過 |
| 8 | add64 の差分試験 4160 件一致・cpuid の食い違いを検出、適格試験(x86_64 45 試験 533 ベクトル、実CPUでも照合) | 通過 |
| 9 | `selfcheck/run.py`: 段階1(cargo test)・段階2(静的 mukoz CLI を native-process で11行の表)・段階3(kernels 7 関数 × 2 ISA、x86_64 は native-routine と差分)。`independence = self`(前の版の判定器がない)なので段階2・3は HOLD `SELF_CHECK_ONLY` で、claim はすべて満たした | 通過(HOLD は設計どおり) |
| 10 | `selfcheck/genloop/2026-10-05/`: 別エージェントが CLI 出力だけで todo の6欠陥を直し、8回の check で REJECT → ACCEPT。回帰ケースが再発を捕まえた回数は 0(再発が起きなかった) | 記録 |

未確認・未達: 継続実行(CI)は未設定。段階2・3の `previous_version` での実行は、前の版の判定器が存在しないため未実施(`selfcheck/checkers.toml` は空)。生成ループ試験は1回・1課題だけ。

## 9.9 測定

既知の不具合の検出率、正しい fixture の誤拒否、検査不能率、反例の再現率、診断までの操作数、返却byte数、CPU時間、最大メモリ、AIの修正までの反復回数を測る。未対応で除外した変異は件数と理由を併記する。性能目標の数値は最初の基準測定の後に決める。
