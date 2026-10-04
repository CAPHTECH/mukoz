# 13. プロセス境界とモジュール分割(mukoz 0.2 の実装)

ルーチン1つを超える成果物(システムコールを使うプログラム、複数のモジュールに分けた実装)を検査する仕組みを定める。本章は**実装済みの範囲**を書く。04・05章の設計と食い違うときは本章(と実装のエラーメッセージ)が正しい。

## 13.1 ねらい

- コマンドラインのアプリケーション(引数・標準入出力・ファイル・終了コード)を契約で書き、Mukoz で検査できるようにする。
- 大きな実装を**モジュールに分けて**、モジュールごとに検査し、組み合わせたときに不具合の**責任の所在**(呼び元か呼び先か)を分けて報告する。

## 13.2 プロセス境界

### ターゲット

`<isa>/<format>/<abi>/linux`。`format` は `raw`(先頭または `[entry] offset` が入口)か `elf`(静的な ET_EXEC。`kind = "elf_entry"`)。

| 項目 | 内容 |
|---|---|
| 対応 | x86-64(sysv-x86_64)・AArch64(aapcs64)。どちらも Linux の規約 |
| ELF | 64ビット・リトルエンディアン・ET_EXEC のみ。PT_INTERP / PT_DYNAMIC は `UNRESOLVED_DEPENDENCY`、ET_DYN(PIE)・PT_TLS は `UNSUPPORTED_FEATURE` で読込み時に止める。セグメントは p_flags の R/W/X どおりに許可する |
| raw | コードは `0x100000` に読み取り専用で置く。書込み可能な領域として `0x10000000` から `[process] data_bytes`(既定 64 KiB、0 で無し)を 0 で埋めて置く |
| 起動時の状態 | Linux と同じ: sp → argc、argv[]、NULL、envp NULL、auxv AT_NULL。文字列はスタック上端。sp は16バイト境界。汎用レジスタは 0 |
| スタック | 既定 64 KiB(`[stack] bytes`) |

### システムコール(作用モデル)

番号は Linux の x86-64 表と asm-generic 表(AArch64)による。呼出しは x86-64 が `syscall`(rax・rdi/rsi/rdx/r10)、AArch64 が `svc`(x8・x0〜x3)。

| 作用 | x86-64 / AArch64 | モデル |
|---|---|---|
| read | 0 / 63 | fd 0 は stdin、それ以外は開いたファイル |
| write | 1 / 64 | fd 1・2 は stdout・stderr、それ以外はファイル |
| open / openat | 2, 257 / 56 | `[files]` で宣言したパスだけ。O_CREAT・O_TRUNC・O_APPEND・O_EXCL を解釈する。宣言のないパスは ENOENT(O_CREAT 付きは EACCES)。dirfd は見ない |
| close | 3 / 57 | |
| lseek | 8 / 62 | SEEK_SET / CUR / END。stdin 等は ESPIPE |
| exit / exit_group | 60, 231 / 93, 94 | 終了コードは下位8ビット |

- 契約の `[effects] allow` に `read`・`write`・`open`・`close`・`lseek` から必要なものを書く。exit は常に許可。許可されない作用の呼出しは `effects.no_forbidden` の違反。
- **上の表にないシステムコールは `UNSUPPORTED_DURING_RUN`(HOLD)**。黙って成功にしない(01章 P5)。
- システムコールに渡したバッファ・パスが許可されたメモリの外なら、その時点でメモリ違反(Linux なら EFAULT を返すが、検査では不具合として止める)。
- ファイルの大きさの上限は、観測する契約の状態変数の `max_len`。超える書込みは ENOSPC。stdout・stderr の上限は結果変数の `max_len` で、超えたら `effects.output_within_limit` の違反。

### 契約と Binding

```toml
# contract.toml
boundary = "process"
modifies = ["db", "db_exists"]
[inputs]   op = "bv8"; nargs = "bv64"; text = { type = "bytes", max_len = 24 }
[state]    db = { type = "bytes", max_len = 600 }; db_exists = "bool"
[results]  code = "bv8"; out = { type = "bytes", max_len = 1024 }; err = { type = "bytes", max_len = 64 }
[effects]  allow = ["read", "write", "open", "close"]
[termination] kind = "must_exit"
```

```toml
# binding.toml
target = "x86_64/elf/sysv-x86_64/linux"
[entry] kind = "elf_entry"
[process]
argv0 = "todo"
argv = ['ite(input.op == bv8(0), b"add", b"list")', "input.text"]   # bytes の式。NUL を含めない
argc = "input.nargs"           # 使う argv の数(argv0 を除く)。argv の数を超えたら BINDING_MISMATCH
stdin = "input.data"           # 省略時は空
[files.db]
path = "todo.db"
init = "before.db"             # 初期内容
exists = "before.db_exists"    # 省略時は true
observe_as = "after.db"        # 終了時の内容(存在しなければ空)
exists_as = "after.db_exists"  # 終了時に存在するか
[results] code = "exit_status"; out = "stdout"; err = "stderr"
[completion] kind = "exit"
```

- プロセスの結果は bytes 型を持てる(stdout・stderr)。
- 機械の性質は `machine.exited`(exit で終わった。入口から ret する等で制御が外れたら違反)・`machine.memory.access`・`effects.no_forbidden`・`effects.output_within_limit`。
- 反例の `observed` に stdout・stderr(テキストと16進)・終了コード・各ファイル・直近16回のシステムコール(引数と戻り値)を出す。

## 13.3 モジュール分割

### リンクファイル

```toml
schema = "mukoz.link/1"

[[modules]]
name = "main"
path = "main.bin"
exports = { start = 0, helper = 0x40 }   # 記号 = モジュール内オフセット

[[modules]]
name = "store"
path = "store.bin"
exports = { load = 0, save = 0x80 }

[imports]            # スロット番号 = "モジュール.記号"
0 = "store.load"
1 = "store.save"

[monitors]           # 呼び先の記号 = その記号の Suite(ルーチンの契約と Binding)
"store.load" = "store_load/suite.toml"
```

- **配置:** i 番目のモジュールを `0x100000 + i × 0x100000` に読み取り専用・実行可能で置く(最大16、各 1 MiB まで)。
- **インポート表:** `0xf0000` に 8 バイトずつ、スロット順に絶対アドレスを入れる(読み取り専用、512 スロット)。再配置は無い。呼出しは表を通す:
  - x86-64: `call qword ptr [0xf0000 + 8*k]`(`ff 14 25 <disp32>`)
  - AArch64: `movz x16, #0xf, lsl #16` → `ldr x16, [x16, #8*k]` → `blr x16`
- Binding に `link = "link.toml"` と `[entry] kind = "symbol"`, `symbol = "main.start"` を書く。ルーチンでもプロセスでも使える(プロセスなら raw のみ)。
- `--artifact <file>` は入口のモジュールのファイルを、`--module <name>=<file>` は任意のモジュールのファイルを差し替える(繰り返し可)。
- 反例の位置は `store+0x1c` のように「モジュール名+オフセット」で出す。

### 開発の進め方

1. 葉のモジュールから作り、それぞれを**単独で** Suite(ルーチンの契約)で検査する(`--artifact` でそのモジュールのファイルを渡す)。
2. 上位のモジュールは、下位の**実物**をリンクして検査する。下位の Suite を `[monitors]` に書くと、呼出しのたびに下位の契約も確かめる。
3. 下位を差し替えて上位だけを確かめるには、正しいと分かっている別実装を `--module` で入れる。契約から自動でスタブを作る機能は未実装(10章)。

## 13.4 境界の監視と責任の所在

`[monitors]` に書いた記号の入口に制御が来るたびに、その記号の Binding を逆に使って契約の値を復元し、契約を評価する。

| 性質 | 意味 | 責任 |
|---|---|---|
| `link.<記号>.requires` | 呼出し時に、呼び先の requires が偽 | **呼び元** |
| `link.<記号>.ensures` | 戻ったときに、呼び先の ensures か frame が偽 | **呼び先** |
| `link.<記号>.abi` | 戻ったときに callee-saved レジスタが変わっていた | **呼び先** |

- 値の復元: 引数が `input.x`(bv・bool)、`len(input.b)`、`addr(region)` の形のときだけ復元できる。領域の大きさは `len(...)` の引数から、それができなければ Binding の領域に `monitor_size = "<引数の式>"` を書く(例: dst の大きさが requires で len(src) と等しいなら `monitor_size = "len(input.src)"`)。復元できない Binding は読込み時に `MONITOR_UNSUPPORTED`。
- 戻りの検出: 呼出し時の戻り先(x86-64 は [rsp]、AArch64 は x30)に、戻り後の sp で到達したとき。再帰・入れ子も追う。
- 呼び先が一度も呼ばれなかったケースでは、その監視の性質を評価しない。全ケースで一度も呼ばれなければ NOT_EVALUATED(HOLD)。
- 違反の詳細(`detail`)に `blame`、呼出し回数、`why_false`、requires 違反では戻り先(`return_to`、呼出し位置の直後)を出す。

**できないこと(未実装):**
- 呼び先が、自分の契約の領域外にある呼び元のメモリ(呼び元のスタック等)を書き換えることの検出。プロセス全体のメモリ監視(許可された領域かどうか)だけが働く。
- 契約からのスタブ生成(呼び先を実装せずに呼び元を検査する)。
- ELF のモジュール分割(動的リンク・再配置)。

## 13.5 確認したこと

| 何を | 結果 |
|---|---|
| hello(x86-64 raw・AArch64 raw・AArch64 ELF) | 3つとも ACCEPT。出力長を変えた変異は `proc.hello/greets` で REJECT(各1回) |
| todo(x86-64 静的 ELF、gcc・libc なし。add/list/clear、ファイル・argv・stderr・終了コード) | 正しい版 ACCEPT、改行を落とす変異・追記しない変異は `add_appends_line` で REJECT、fstat を呼ぶ変異は HOLD(UNSUPPORTED_DURING_RUN)(各1回) |
| PIE の ELF(ホストの /bin/true) | 読込み時に拒否(終了コード 2) |
| リンク: sum3 → add64(x86-64・AArch64) | 正しい組み合わせ ACCEPT。呼び先を引き算にすると `link.arith.add64.ensures`(呼び先)、rbx を壊すと `.abi`(呼び先)、呼び先の requires を厳しくすると `.requires`(呼び元、`return_to = main+0x18`) |
| リンク: copy_twice → copy(領域の復元) | 正しい版 ACCEPT。半分だけコピーする呼び先は `link.mem.copy.ensures`、1バイト余分に読む呼び先はメモリ違反で、位置は `mem+0x…` |

上はすべて `tests/acceptance.rs` に固定した。最初の試行で、正しい todo が REJECT になった。原因は2つで、どちらも Mukoz が見つけた:

1. ファイルが上限まで埋まっているときの ENOSPC を、プログラムが無視していた。対処として、契約に前提を足した。
2. argv の式の数を超える argc を、Mukoz が黙って切り詰めていた。これは Mukoz の欠陥で、BINDING_MISMATCH に直した。
