# WP0.5b 正規化イベント → Mukoz external claims

正規化した観測イベントを、`mukoz check --external-claims`(WP0.5a、branch `wp05-external-claims`)が読む `mukoz.external_claims/0` に変える(DESIGN.md 16.1、17章 WP0.5b)。イベントから claim への変換は Mukoz の外に置く(2026-10-19 の決定)。2026-10-19 に、人工のイベントで 7.5 の負の試験と end-to-end を通した。**strace の記録から正規化イベントを作る部分は `normalize/`(WP3)にある。Memcheck の部分(WP4)はまだ無い。**

```
adapter.py --binding B --observations O --subject-context HEX --independence {independent,previous_version,self,undeclared} -o OUT
   → OUT(mukoz.external_claims/0)と OUT.receipt.json(binding・observations の sha256、collector)
   入力の形が誤っていれば exit 2 で、何も書かない
```

## 入力

- **binding**(`secobs.binding/0`): resource の目録、principal ごとの許可(read/write)、許可された接続先と実行ファイル、出力する `external.*` property と述語の対応、既存 property を埋める `fills`(省略可)。
- **observations**(`secobs.observations/0`): binding の sha256、case ごとの principal・送った request 数・receipt(観測の開始と終了、欠落、失ったイベント数、使えた capability)・イベント列。イベントの形は 10.3 の例に、転送先(`dest_resource_id`)・open の access・Memcheck の種類(`detail.kind`)を足したもの。

## 判定の規則(property ごと、case ごと)

1. 述語に要る capability(fs・net・proc・memcheck)が receipt に無い → `NOT_EVALUATED` / `REQUIRED_CAPABILITY_UNAVAILABLE`
2. observations の binding sha256 が binding と違う → `INCONCLUSIVE` / `BINDING_STALE`(反例も出さない)
3. 証拠として数えるのは、origin が `os_observer` か `memcheck` で、subject が `sut` のイベントだけ。**SUT の出力(`sut_output`)と supervisor の作用は、違反の証拠にも成功の証拠にもしない。**入力のどの値も、eval へそのまま写さない
4. case への帰属が確実(`exclusive_process_group`・`exclusive_namespace`)で、resource が解決済みの反例が1つでもある → `VIOLATED`。観測の欠落があっても消さない
5. それ以外で、次のどれかがある → `INCONCLUSIVE`(理由は最初の1つ): 開始が SUT の exec より前でない、終了が drain 済みでない、欠落か失ったイベントがある、request を1つも送っていない(`VACUOUS_CASE`)、未対応のイベント種別、未対応の経路(`unsupported_mechanism` で、resource が不明か保護対象に関わるもの。`UNSUPPORTED_MECHANISM`)、resource か帰属が未解決で違反かどうか決まらないイベント
6. 残り → `SATISFIED_IN_SCOPE`

述語 `mem.region.read_authorized`(WP4 で追加): 1つの有効な allocation の中の論理領域の読み(F12)。capability `region` を要求し、SEC-OBS-01 のどの collector もそれを出さないので、Memcheck があっても常に `NOT_EVALUATED` になる。Memcheck が何も報告しなくても SATISFIED にしないためである。

述語: 禁止 resource の open 成功(access 不明なら read と write の両方で照合)、正の byte 数の read(sendfile 等の転送は転送元の read)、正の byte 数の write(転送は転送先の write)、許可外への接続の試行・成立、許可外の exec、Memcheck の不正アクセス(リークは含まない)。目録に無い resource(共有ライブラリ等)は policy の対象外。

`fills` は、既存 property(`effects.no_forbidden` 等)を複数の external 結果の論理積で埋める。どれかが VIOLATED なら VIOLATED、全部 SATISFIED なら SATISFIED、それ以外は INCONCLUSIVE か NOT_EVALUATED。**[R] 意味のずれ**: Mukoz の `effects.no_forbidden` は suite の契約が定める禁止作用で、binding の policy とは別に定義されている。`machine.memory.access` は許可された読取り領域(`docs/06` 6.2)で、Memcheck のヒープ検査より広い。`e2e.py` では ACCEPT まで通すために両方を埋めたが、本試験でそう埋めてよいかは未決定。

## 試験

```sh
python3 test_adapter.py                      # 48 本。7.5 の各項目と、形の誤り
python3 mutate.py                            # adapter.py を 26 通り壊し、どれも試験が落ちることを確かめる
python3 e2e.py <mukoz の worktree>           # 実際の mukoz check --gate を 15 通り
```

2026-10-19 の結果(各1回。WP3 で未対応の経路のイベント `unsupported_mechanism` と `O_PATH` を足した後に再実行した):

- `test_adapter.py`: 45 本すべて通った。
- `mutate.py`: exit 0。壊していない adapter では通り、23 通りの壊し方(0 byte を read と数える、失敗した read を数える、sendfile を数えない、principal を無視する、SUT の出力を信じる、supervisor の作用を数える、開始・終了・欠落・空の範囲・binding の失効を見ない、未解決を許可扱いにする、欠測で反例を落とす、など)はすべて1本以上の試験を落とした。
- `e2e.py`(Mukoz `0eeb529` の release build、native-process の hello suite、4 case、case ごとに新しい store): exit 0。15 通りすべて期待どおりだった。

| 場面 | 結果 |
|---|---|
| フラグ無し(対照) | HOLD(exit 10) |
| 全 case が清浄 | ACCEPT_WITHIN_SCOPE(0) |
| 禁止 resource の read 32 byte / read = -1(失敗) | REJECT(11) / ACCEPT(0) |
| sendfile 4096 byte / 0 byte | REJECT / ACCEPT |
| 同じ read を許可 principal(bob) / 禁止 principal(alice) | ACCEPT / REJECT |
| 1 case の観測開始が欠落 | HOLD(`MONITORING_START_MISSING`) |
| 反例の後でイベントを失い、別 case は終了が欠落 | REJECT |
| SUT が `PASS` と偽の claims JSON を出し、観測開始は欠落 | HOLD |
| 1 case の観測が無い / case が1つも無い | HOLD(external property の欠測、元の `REQUIRED_CAPABILITY_UNAVAILABLE`) |
| 全 case で request を送っていない | HOLD(`VACUOUS_CASE`) |
| 独立性を宣言しない | HOLD(`EXTERNAL_INDEPENDENCE_UNDECLARED`) |
| subject context の 1 文字違い | exit 2 `STALE_CONTEXT` |

## やっていないこと

- **Memcheck の記録から正規化イベントを作る部分**(WP4)。strace の記録からは `normalize/`(WP3)で作る。この README の試験はどれも人工のイベントを使う。
- **Mukoz 側の `VACUOUS_SCOPE`**: 7.5 の「全入力を事前条件外にする」は Mukoz の `requires` による絞り込みで起きる。ここで試したのは、ハーネス側の対応物(case が無い、request を送っていない)だけ。
- **phase と観測期間の照合**: イベントの `phase` は読んでいない。期間は receipt の開始・終了だけで見ている。
- **request への帰属**(9.4)と、一つの case の中の複数 request の区別。
- 別系列のモデルによるレビュー。この変換器は誰にもレビューさせていない。

WP4 で `mem.region.read_authorized` を足した後に再実行した(2026-10-19、各1回): `test_adapter.py` 47 本すべて通り、`mutate.py` は 25 通りすべてを検出した(足した2通りは「Memcheck で領域を判定する」「許可されない領域の読みを通す」)。

E1 の試し走りで、非ブロッキング connect(`EINPROGRESS` の後を追わない)を `net.no_forbidden_connect` の SATISFIED にしていた誤りを直した(禁止先なら INCONCLUSIVE `OUTCOME_UNOBSERVED`)。直した後に再実行した(2026-10-19、各1回): `test_adapter.py` 48 本すべて通り、`mutate.py` は 26 通りすべてを検出した。
