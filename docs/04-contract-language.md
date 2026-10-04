# 04 契約・Binding・Suite

## 4.1 3つを別々に書く

```text
Contract: aとbの64bit wrapping加算が返る。            (ISA非依存)
Binding:  x86_64 SysV なら a=rdi, b=rsi, 結果=rax。  (プラットフォーム依存)
Suite:    境界値の直積と seed 固定のランダム 4096 件。 (検査方法)
```

- Contractはレジスタ割当を知らない。同じContractに、ISAごとのBindingを複数付けられる。
- Bindingは何が正しいかを定義しない。
- Suiteは必須claimを削らない。

## 4.2 ファイル形式

- 人が書く形式は **TOML**。機械間の正本は **JSON**。どちらも同じ内部モデルへ変換し、正規化したJSONのdigestで識別する。
- 未知のフィールド、重複キーを拒否する。
- 式はTOML/JSON内の**文字列**として書き(4.3)、読込み時に型付きASTへ変換する。正規化JSONには式文字列ではなくASTを保存する。
- 各ファイルは `schema = "mukoz.contract/1"` のように版を持つ。

## 4.3 式言語

任意のPython・JavaScript・shellは許さない。小さな型付き式だけを解釈する。

### 型

| 型 | 意味 |
|---|---|
| `bool` | 真偽 |
| `bv8` `bv16` `bv32` `bv64` | 幅付きビット列。算術はその幅での剰余算 |
| `bytes` | 可変長のバイト列。宣言時に `max_len` が必須 |

数学的整数・浮動小数点・128bit以上は後の版で追加する。

### 構文

```text
expr    := or_expr
or_expr := and_expr ("or" and_expr)*
and_expr:= not_expr ("and" not_expr)*
not_expr:= "not" not_expr | cmp
cmp     := bitor (("==" | "!=") bitor)?
bitor   := bitxor ("|" bitxor)*
bitxor  := bitand ("^" bitand)*
bitand  := add ("&" add)*
add     := mul (("+" | "-") mul)*
mul     := unary ("*" unary)*
unary   := "~" unary | postfix
postfix := primary ("[" expr "]")*
primary := path | literal | call | "(" expr ")"
        | "forall" IDENT "in" expr ".." expr ":" expr
path    := IDENT ("." IDENT)*
call    := IDENT "(" (expr ("," expr)*)? ")"
literal := "true" | "false"
        | "bv" WIDTH "(" (HEX | DEC) ")"      # bv64(0xff), bv8(10)
        | 'b"' ... '"'                         # bytes, \n \xNN のエスケープ
        | 'hex"' HEXDIGITS '"'                 # bytes
```

- `+ - * & | ^ ~` は同じ幅の bv 同士だけに適用する。**暗黙の幅変換をしない。**
- 大小比較は符号の解釈を必ず名前で書く: `ult ule ugt uge slt sle sgt sge`。`<` 記号は持たない。
- その他の関数: `ite(c, a, b)`、`zext(x, 64)`、`sext(x, 64)`、`extract(x, hi, lo)`、`shl(x, n)`、`lshr(x, n)`、`ashr(x, n)`、`len(b)`(bv64)、`slice(b, off, n)`、`concat(a, b)`。
- `b[i]` は bytes の i 番目(bv8)。範囲外は評価エラーで、成功にも失敗にもしない(`EVALUATION_ERROR` → そのclaimは INCONCLUSIVE)。
- `forall i in lo..hi: p` は半開区間 `[lo, hi)` の有界量化。`i` は bv64。範囲の長さは Suite の上限(既定 65,536)を超えてはならない。
- `==` は bv・bool・bytes に使える。bytes の比較は長さと全バイトの一致。

### 変数の名前空間

| 名前空間 | 意味 |
|---|---|
| `input.*` | 入力。実行開始時の不変値 |
| `before.*` | 状態の開始時の値 |
| `after.*` | 状態の終了時の値 |
| `result.*` | ルーチンの戻り値 |
| `stdout` `stderr` `exit.*` | プロセス境界の観測(4.5) |

式の評価で対象を再実行したり、ホストのファイル・環境変数を読んだりしない。式の深さ64、ノード数8,192を上限とする。

## 4.4 ルーチン境界の契約

### 例1: 64bit wrapping加算

```toml
schema = "mukoz.contract/1"
id = "arith.add64"
boundary = "routine"

[inputs]
a = "bv64"
b = "bv64"

[results]
value = "bv64"

[[ensures]]
id = "sum"
expr = "result.value == input.a + input.b"

[effects]
allow = []

[termination]
kind = "must_return"
```

性質の完全なIDは `arith.add64/sum`。`requires` を省略すると「常に真」。

### 例2: バッファコピー(可変長)

```toml
schema = "mukoz.contract/1"
id = "mem.copy"
boundary = "routine"
modifies = ["dst"]

[inputs]
src = { type = "bytes", max_len = 256 }

[state]
dst = { type = "bytes", max_len = 256 }

[[requires]]
id = "same_len"
expr = "len(before.dst) == len(input.src)"

[[ensures]]
id = "copied"
expr = "after.dst == input.src"

[effects]
allow = []

[termination]
kind = "must_return"
```

`modifies` はトップレベルのキーなので、TOMLではどのテーブル見出しよりも前に書く。`modifies` にない状態は、終了時に開始時と等しいことを自動で claim にする(`frame`)。

### 例3: 状態付きカウンタ(失敗系を明示)

```toml
schema = "mukoz.contract/1"
id = "counter.checked_inc"
boundary = "routine"
modifies = ["count"]

[inputs]
n = "bv64"

[state]
count = "bv64"

[results]
status = "bv32"

[[ensures]]
id = "ok_path"
expr = "(result.status == bv32(0)) == (not ult(before.count + input.n, before.count))"

[[ensures]]
id = "ok_updates"
expr = "ite(result.status == bv32(0), after.count == before.count + input.n, after.count == before.count)"

[effects]
allow = []

[termination]
kind = "must_return"
```

overflow時に「状態を変えず非0を返す」ことを契約に書いている。書いていなければ、その振る舞いは検査しない(01章 1.4)。

## 4.5 プロセス境界の契約

0.4には、プログラムのstdoutや終了コードを契約に書く手段がなかった。`boundary = "process"` を加える。

| 観測 | 型 |
|---|---|
| `stdout` `stderr` | bytes(`max_len` は Suite の出力上限) |
| `exit.exited` | bool(正常終了したか) |
| `exit.code` | bv32(OSの終了コード。Linuxでは下位8bitのみ意味を持つ) |
| `exit.signaled` / `exit.signal` | bool / bv32(シグナルで終わった場合) |
| `input.stdin` | bytes |

```toml
schema = "mukoz.contract/1"
id = "hello.smoke"
boundary = "process"

[inputs]
stdin = { type = "bytes", max_len = 0 }

[[ensures]]
id = "stdout"
expr = 'stdout == b"Hello from raw x86-64 ELF!\n"'

[[ensures]]
id = "exit_zero"
expr = "exit.exited and exit.code == bv32(0)"

[[ensures]]
id = "no_stderr"
expr = "len(stderr) == bv64(0)"

[effects]
allow = ["write:1", "exit"]
```

### 作用の語彙

`effects.allow` は、境界の外へ出る作用のうち許すものを列挙する。それ以外は禁止。

| 作用 | 意味 |
|---|---|
| `write:<fd>` | 指定fdへの書込み |
| `read:<fd>` | 指定fdからの読込み |
| `exit` | プロセスの終了 |
| `mem:<region>` | ルーチンが指定領域(Binding)外へ作用を持つこと。通常は書かない |

禁止作用は「試みたこと」自体を違反にする(06章 6.3)。作用を観測できないExecutorでは、このclaimは NOT_EVALUATED になる。

### 頑健性の契約は別に書く

helloの例は「writeが必ず全量成功する環境」での smoke 契約である。短いwriteや失敗が起きる環境で「全量書くか、規定のエラーで終わる」ことは別の契約・別の環境モデルで調べる。smoke契約の成功から頑健性を主張しない。

## 4.6 Binding

### ルーチン(x86_64 SysV)

```toml
schema = "mukoz.binding/1"
id = "arith.add64@x86_64-sysv"
contract = "arith.add64"
target = "x86_64/raw/sysv-x86_64/none"

[entry]
kind = "raw_offset"
offset = 0

[arguments]
rdi = "input.a"
rsi = "input.b"

[results]
value = "rax"

[stack]
bytes = 16384

[completion]
kind = "return_to_sentinel"
```

### 同じ契約の AArch64 Binding

```toml
schema = "mukoz.binding/1"
id = "arith.add64@aarch64-aapcs64"
contract = "arith.add64"
target = "aarch64/raw/aapcs64/none"

[entry]
kind = "raw_offset"
offset = 0

[arguments]
x0 = "input.a"
x1 = "input.b"

[results]
value = "x0"

[stack]
bytes = 16384

[completion]
kind = "return_to_sentinel"
```

### code領域

- rawルーチンで `code_regions` を省略すると、**ファイル全体を1つのcode領域**とする。AIが生成のたびに命令長を変えても、Bindingを書き直さずに済む。
- 命令とデータを同じファイルに置く場合は `[[code_regions]]`(`offset`、`size`)と `[[data_regions]]` を明示する。サイズはファイル長を超えてはならない(`BINDING_MISMATCH`)。
- Bindingは**どのファイルを検査するかを持たない**。成果物はSuiteかCLIで指定する(4.7)。1つのBindingを、生成のたびに変わる成果物へ使い回すためである。

### 領域を使うルーチン(mem.copy、x86_64 SysV)

```toml
schema = "mukoz.binding/1"
id = "mem.copy@x86_64-sysv"
contract = "mem.copy"
target = "x86_64/raw/sysv-x86_64/none"

[entry]
kind = "raw_offset"
offset = 0

[regions.dst]
size = "len(before.dst)"
init = "before.dst"
access = "rw"
observe_as = "after.dst"

[regions.src]
size = "len(input.src)"
init = "input.src"
access = "r"

[arguments]
rdi = "addr(dst)"
rsi = "addr(src)"
rdx = "len(input.src)"

[completion]
kind = "return_to_sentinel"
```

- ハーネスは各領域を別の位置に置き、間に**未割当のguard領域**を挟む。加えてbyte単位のアクセス監視で、領域外アクセスを検出する。
- 領域の配置(alignment・相対位置)は Suite の生成対象にできる(4.7)。
- ポインタ引数は入口時の値を固定し、後でレジスタが上書きされても再解決しない。状態の読出しは `observe_as` で領域を指す。
- 領域はそれぞれ、mapped・初期化済み・読取り可・書込み可・終了時に不変、を別々に指定できる。既定は `access` から決める。

### Bindingが自動で加える machine claim

| claim | 内容 |
|---|---|
| `machine.returned` | 入口時に用意した復帰先へ、正しいSPで戻った(05章 5.3) |
| `machine.abi.callee_saved` | ABIの保存レジスタが入口時と等しい |
| `machine.abi.stack` | SPの復元、規定地点でのalignment |
| `machine.abi.reserved` | 予約レジスタを使わない(例: Apple / Windows ARM64 の x18) |
| `machine.memory.access` | すべてのアクセスが許可領域に完全に収まる |
| `effects.no_forbidden` | 禁止作用の試行がない |

ABIごとの内容は 05章 5.4 の表で決める。

### プロセス(x86_64 Linux ELF)

```toml
schema = "mukoz.binding/1"
id = "hello.smoke@x86_64-linux"
contract = "hello.smoke"
target = "x86_64/elf/sysv-x86_64/linux"

[entry]
kind = "format_entry"          # ELF の e_entry

[process]
argv = ["hello"]
env = {}

[environment]
model = "linux-stdio/1"
write_results = "full-success/1"
```

## 4.7 Suite

```toml
schema = "mukoz.suite/1"
id = "arith.add64.primary"
contract = "arith.add64"
binding = "arith.add64@x86_64-sysv"
executors = ["emulated"]

[artifact]
path = "build/add64.bin"      # Suiteファイルからの相対パス。CLIの --artifact で上書きできる

[generate]
seed = "20261004"
boundary = "product"          # 型ごとの既定境界値の直積
random_cases = 4096

[limits]
instructions_per_case = 100000
wall_ms_per_case = 1000
guest_memory_bytes = 16777216
trace_bytes_per_case = 4194304
```

- `[artifact]` の `path` は所在を示すだけで、同一性は読み込んだ時点のスナップショットの digest で決まる。Planはその digest に結び付き、実行の直前・直後に digest を検査する(08章 8.5)。
- `[artifact]` も `--artifact` もない場合は `CONTRACT_GAP` ではなく使い方の誤り(終了コード2)とする。
- bv64 の既定境界値は `0, 1, 2, 0x7fff…ffff, 0x8000…0000, 0xffff…fffe, 0xffff…ffff, 0x0101…0101` の8個。直積なら2入力で64件、ランダムと合わせて4,160件。重複を除かず、生成順をcase IDに含める。重複入力数は報告し、「4,160個の異なる入力」とは書かない。
- 境界値は `[generate.vars.<変数名>]` で追加・制御できる(実装済みの項目は 4.10)。
- bytes は長さ0・1・`max_len`・ランダム長を既定とする。
- 必須claimは既定で「Contractの全ensures + frame + Bindingのmachine claim」。Suiteは追加はできるが削除はできない。削除はPolicyでのみ行い、その事実を証跡に残す。
- **回帰ケース:** 過去の反例は回帰ケースとして自動で加わる(4.8)。`[regressions] include = false` で外せるが、外したことを証跡と出力の `limitations` に記録する。
- `executors` に複数を書くと、同じケースを複数のExecutorで実行し、差分を比べる(06章 6.8)。
- 上限がPolicyの上限を超える場合は計画を拒否し、黙って切り詰めない。
- 有効なケース(事前条件を満たすもの)が0件なら HOLD(`VACUOUS_SCOPE`)。
- 有効なケースが下限未満なら HOLD(`LOW_ADMITTED_CASES`)。下限の既定は min(100, 生成数の1/4)。事前条件がほとんどのケースを捨てるのは、生成器が契約の想定する入力を作れていない兆候であり、残りの数件での合格は範囲を過大に見せるため。意図して絞る場合は `[limits] min_admitted_cases` で下限を明示する。捨てた件数は常に `limitations` に出す。

## 4.8 回帰ケース

AIが生成と検査を繰り返すとき、前に見つかった失敗が再発していないかを毎回確かめるため、反例を**成果物ではなく契約とターゲットに結び付けて**ためる。

```text
.mukoz/regressions/<contract digest>/<target platform>/<case digest>.json
```

- 保存する内容: 意味上の入力(`input.*`・`before.*`)、環境応答のスクリプト、領域の配置、Bindingが指定しなかったレジスタの初期値、元の反例ID、縮小済みかどうか。
- 縮小済みの反例があればそれを、なければ元の反例を入れる。両方を持つ場合は両方入れる。
- 次の `plan` / `check` では、回帰ケースを生成ケースより**先に**並べ、case IDを `reg-` で始める。件数は生成ケースと別に報告する。
- Bindingが変わっても、契約とターゲットが同じなら使える。ABIが同じならレジスタの初期値もそのまま使う。Bindingの変更で回帰ケースを当てはめられない場合(必要な変数がない等)は、そのケースを NOT_EVALUATED として報告し、黙って捨てない。
- 契約の digest が変わったら、古い回帰集合は**当てはまらないもの**として件数だけ報告する。新しい契約へ自動では移さない(期待値の意味が変わり得るため)。
- 回帰ケースは自動では消さない。上限(既定1,024件)を超えたら計画を拒否し(`REGRESSION_LIMIT_EXCEEDED`)、黙って間引かない。整理は `mukoz regressions prune` で明示的に行い、その操作を記録する。
- 回帰ケースが通ったことは「その反例が再発していない」という意味であり、全体の正しさではない(06章 6.10)。

## 4.9 契約の不足と矛盾

| 状況 | 報告 |
|---|---|
| 必要な情報がない(型、完了条件等) | `CONTRACT_GAP` |
| 型が合わない | `CONTRACT_TYPE_ERROR` |
| 生成器が事前条件を満たすケースを作れない | `VACUOUS_SCOPE`(論理的矛盾とは断定しない) |
| Binding と Contract の変数が対応しない | `BINDING_MISMATCH` |

## 4.10 実装状況(mukoz 0.1)

この章は設計であり、実装はその部分集合である。**食い違うときは実装(ツールのエラーメッセージ)が正しい。**契約を書くエージェントのために、実装済みの範囲をここに固定する。

| 項目 | 0.1 の実装 |
|---|---|
| Suite の `contract` / `binding` | **ファイルパス**(Suite からの相対)。ID 参照は未実装 |
| `executors` | `["emulated"]` のみ |
| `[generate]` | `seed`・`boundary`(`product` / `none`)・`random_cases` |
| `[generate.vars.<名前>]` | `values`: 追加の値(bv は 10進/0x16進の文字列、bytes は16進文字列。`hex"..."` 形式ではない)。`len`: bytes の長さの式(前の変数を参照可。例 `len(input.src) + len(input.src)`)。`max`: bv の上限の式(含む。前の変数を参照可)。`bytes`: bytes の値域 `nonzero` / `ascii`。型に合わない項目はエラー |
| 依存する生成 | `len` / `max` が参照する変数を先に生成する(循環はエラー)。`len` が契約の `max_len` を超えたら `PLAN_ERROR`(切り詰めない) |
| `[limits]` | `instructions_per_case`・`wall_ms_per_case`・`max_cases`(上限 8192)・`min_admitted_cases`。`guest_memory_bytes` / `trace_bytes_per_case` は未実装 |
| 領域の配置の生成(alignment・相対位置) | 未実装。各領域は別々の固定番地に置く |
| machine claim | `machine.returned`(SP の復元を含む)・`machine.abi.callee_saved`・`machine.abi.flags`(x86 の DF)・`machine.abi.reserved`(apple-arm64 の x18 のみ)・`machine.memory.access`・`effects.no_forbidden`。`machine.abi.stack` は独立の claim ではなく `machine.returned` に含む |
| 式 | 4.4 の型付き式。整数リテラルは `bvN(...)` で幅を明示する(契約内の裸の整数は `CONTRACT_TYPE_ERROR`)。`mukoz expr check` は構文だけを検査し、型は契約の読込み時に検査する |
| `forall` / `count` の範囲 | 1式あたり 65,536 まで |
| プロセス Binding・環境モデル | 未実装(境界は `routine` のみ) |

