"""Generate the modular todo task: per-routine contracts/bindings/suites under check/<m>/,
link files, and the main process suite (contract = ../todo/contract.toml, reused).
usage: python3 make.py <out-dir>"""
import os, shutil, sys

OUT = sys.argv[1]
HERE = os.path.dirname(os.path.abspath(__file__))
R = 72
Z64 = 'hex"' + "00" * 64 + '"'
MAX = "bv64(0xffffffffffffffff)"
MODS = ["parse_id", "udec", "find_rec", "fmt_line", "make_rec", "del_rec", "clear_done"]   # import slot = index

def rid(db, i): return f"u32le({db}, {i} * bv64({R}))"
def rec(db, i): return f"slice({db}, {i} * bv64({R}), bv64({R}))"
def flag(db, i): return f"{db}[{i} * bv64({R}) + bv64(4)]"
def tlen(db, i): return f"zext({db}[{i} * bv64({R}) + bv64(5)], 64)"
def text(db, i): return f"slice({db}, {i} * bv64({R}) + bv64(8), {tlen(db, i)})"

# Well-formed record list from generator parameters (same shape as ../todo/suite.toml).
REC_PARAMS = {"nrec": "bv64", "base": "bv32", "gap": "bv8", "flags": "bv64", "lsel": "bv8", "pool": "{ type = \"bytes\", max_len = 120 }"}
def recx(i="i"):
    idv = f"(zext(input.base, 64) + bv64(1) + {i} * (zext(input.gap, 64) + bv64(1)))"
    L = f"(bv64(1) + urem({i} * bv64(7) + zext(input.lsel, 64), bv64(64)))"
    return (f"concat(le_bytes(extract({idv}, 31, 0)), concat(le_bytes(extract(lshr(input.flags, {i}) & bv64(1), 7, 0)), concat(le_bytes(extract({L}, 7, 0)), "
            f"concat(hex\"0000\", slice(concat(slice(input.pool, {i}, {L}), {Z64}), bv64(0), bv64(64))))))")
def dbx(count): return f"(join i in bv64(0)..{count}: {recx()})"
REC_GEN = '''
[generate.vars.base]
max = "bv32(4294950000)"
values = ["0", "4294950000"]

[generate.vars.pool]
len = "bv64(120)"
bytes = "ascii"
'''

def w(path, s):
    p = os.path.join(OUT, path)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "w").write(s)

def contract(cid, inputs, state, results, requires, ensures, modifies=()):
    s = f'schema = "mukoz.contract/1"\nid = "{cid}"\nboundary = "routine"\nmodifies = {list(modifies)!r}\n'.replace("'", '"')
    s += "\n[inputs]\n" + "".join(f"{k} = {v if v.startswith('{') else chr(34) + v + chr(34)}\n" for k, v in inputs.items())
    if state:
        s += "\n[state]\n" + "".join(f"{k} = {v}\n" for k, v in state.items())
    if results:
        s += "\n[results]\n" + "".join(f'{k} = "{v}"\n' for k, v in results.items())
    for i, e in requires:
        s += f"\n[[requires]]\nid = \"{i}\"\nexpr = '''{e}'''\n"
    for i, e in ensures:
        s += f"\n[[ensures]]\nid = \"{i}\"\nexpr = '''{e}'''\n"
    return s + '\n[effects]\nallow = []\n\n[termination]\nkind = "must_return"\n'

def binding(cid, args, regions, results, link=None, symbol=None):
    s = f'schema = "mukoz.binding/1"\nid = "{cid}@x86_64"\ncontract = "{cid}"\ntarget = "x86_64/raw/sysv-x86_64/none"\n'
    if link:
        s += f'link = "{link}"\n\n[entry]\nkind = "symbol"\nsymbol = "{symbol}"\n'
    else:
        s += '\n[entry]\nkind = "raw_offset"\noffset = 0\n'
    s += "\n[arguments]\n" + "".join(f'{r} = "{e}"\n' for r, e in args.items())
    for name, r in regions.items():
        s += f"\n[regions.{name}]\n" + "".join(f'{k} = "{v}"\n' for k, v in r.items())
    if results:
        s += "\n[results]\n" + "".join(f'{k} = "{v}"\n' for k, v in results.items())
    return s + '\n[completion]\nkind = "return_to_sentinel"\n'

def suite(sid, gen, cases=600):
    return (f'schema = "mukoz.suite/1"\nid = "{sid}"\ncontract = "contract.toml"\nbinding = "binding.toml"\n\n[generate]\nseed = "{sid}"\nrandom_cases = {cases}\n'
            f"{gen}\n[limits]\ninstructions_per_case = 200000\n")

# 1. parse_id(rdi = s, rsi = len) -> rax: value, or all ones when not a valid ID
n = "len(input.s)"
s_ = "input.s"
digits = f"(forall i in bv64(0)..{n}: (uge({s_}[i], bv8(0x30)) and ule({s_}[i], bv8(0x39))))"
synt = f"(ugt({n}, bv64(0)) and ule({n}, bv64(10)) and {digits})"
zs = f"(count i in bv64(0)..{n}: (forall j in bv64(0)..(i + bv64(1)): {s_}[j] == bv8(0x30)))"
t = f"slice({s_}, {zs}, bv64(10))"
u = 'b"4294967295"'
over = f"ite(({n} - {zs}) == bv64(10), ugt((count i in bv64(0)..bv64(10): (slice({t}, bv64(0), i) == slice({u}, bv64(0), i) and ugt({t}[i], {u}[i]))), bv64(0)), false)"
dv = "dec(result.v)"
ok = f"(ule(result.v, bv64(0xffffffff)) and ite(ule(len({dv}), {n}), {s_} == concat((join k in bv64(0)..({n} - len({dv})): b\"0\"), {dv}), false))"
w("check/parse_id/contract.toml", contract("todo.parse_id", {"s": '{ type = "bytes", max_len = 16 }'}, {}, {"v": "bv64"}, [],
  [("value_or_all_ones", f"ite({synt}, ite({over}, result.v == {MAX}, {ok}), result.v == {MAX})")]))
w("check/parse_id/binding.toml", binding("todo.parse_id", {"rdi": "addr(s)", "rsi": "len(input.s)"}, {"s": {"size": "len(input.s)", "init": "input.s", "access": "r"}}, {"v": "rax"}))
w("check/parse_id/suite.toml", suite("parse_id", '''
[generate.vars.s]
pieces = ["30:6", "31", "32", "34", "35", "36", "39:2", "2f", "3a", "61", "20"]
values = ["34323934393637323935", "34323934393637323936", "30303030303030303031", "3030303030303030303031", "30", "3939393939393939393939", "3030303034323934393637323935", "3432393439363732393530"]
''', 1500))

# 2. udec(edi = value, rsi = out) -> rax = number of bytes written (decimal, no leading zeros)
d = "dec(input.v)"
w("check/udec/contract.toml", contract("todo.udec", {"v": "bv32"}, {"out": '{ type = "bytes", max_len = 16 }'}, {"n": "bv64"},
  [("out_16", "len(before.out) == bv64(16)")],
  [("decimal", f"result.n == len({d}) and slice(after.out, bv64(0), len({d})) == {d} and slice(after.out, len({d}), bv64(16) - len({d})) == slice(before.out, len({d}), bv64(16) - len({d}))")],
  modifies=["out"]))
w("check/udec/binding.toml", binding("todo.udec", {"rdi": "input.v", "rsi": "addr(out)"},
  {"out": {"size": "len(before.out)", "monitor_size": "bv64(16)", "init": "before.out", "access": "rw", "observe_as": "after.out"}}, {"n": "rax"}))
w("check/udec/suite.toml", suite("udec", '''
[generate.vars.v]
values = ["9", "10", "99", "100", "999999999", "1000000000", "4294967295", "4294967294"]

[generate.vars.out]
len = "bv64(16)"
''', 800))

# 3. find_rec(rdi = db, rsi = n, edx = id) -> rax = index of the record with that id, or all ones
db = "input.db"
cnt = "input.n"
found = f"ugt((count i in bv64(0)..{cnt}: {rid(db, 'i')} == input.id), bv64(0))"
w("check/find_rec/contract.toml", contract("todo.find_rec",
  {"db": '{ type = "bytes", max_len = 3600 }', "n": "bv64", "id": "bv32", "kind": "bv8", "pick": "bv64", "idr": "bv32", **REC_PARAMS}, {}, {"k": "bv64"},
  [("n_records", f"ule({cnt}, bv64(50)) and len({db}) == {cnt} * bv64({R})")],
  [("index_or_all_ones", f"ite({found}, ite(ult(result.k, {cnt}), {rid(db, 'result.k')} == input.id, false), result.k == {MAX})")]))
w("check/find_rec/binding.toml", binding("todo.find_rec", {"rdi": "addr(db)", "rsi": "input.n", "rdx": "input.id"},
  {"db": {"size": "len(input.db)", "monitor_size": f"input.n * bv64({R})", "init": "input.db", "access": "r"}}, {"k": "rax"}))
w("check/find_rec/suite.toml", suite("find_rec", f'''
[generate.vars.nrec]
max = "bv64(50)"

[generate.vars.kind]
max = "bv8(3)"
{REC_GEN}
[generate.vars.db]
expr = \'\'\'{dbx("input.nrec")}\'\'\'

[generate.vars.n]
expr = "udiv(len(input.db), bv64({R}))"

[generate.vars.id]
expr = \'\'\'ite(input.kind == bv8(0) and ugt(input.n, bv64(0)), {rid("input.db", "urem(input.pick, input.n)")}, ite(input.kind == bv8(1), extract(zext(input.base, 64) + bv64(2), 31, 0), ite(input.kind == bv8(3) and ugt(input.n, bv64(0)), {rid("input.db", "urem(input.pick, input.n)")} ^ bv32(0x10000), input.idr)))\'\'\'
''', 800))

# 4. fmt_line(rdi = record, rsi = out) -> rax = length of "<id> [ ] <text>\n" / "<id> [x] <text>\n" written to out
r1 = "input.rec"
line = f'concat(dec({rid(r1, "bv64(0)")}), concat(ite({flag(r1, "bv64(0)")} == bv8(1), b" [x] ", b" [ ] "), concat({text(r1, "bv64(0)")}, b"\\n")))'
w("check/fmt_line/contract.toml", contract("todo.fmt_line", {"rec": '{ type = "bytes", max_len = 72 }', **REC_PARAMS}, {"out": '{ type = "bytes", max_len = 80 }'}, {"n": "bv64"},
  [("one_record", f"len({r1}) == bv64({R}) and len(before.out) == bv64(80) and ugt({tlen(r1, 'bv64(0)')}, bv64(0)) and ule({tlen(r1, 'bv64(0)')}, bv64(64)) and ule({flag(r1, 'bv64(0)')}, bv8(1))")],
  [("line", f"result.n == len({line}) and slice(after.out, bv64(0), len({line})) == {line} and slice(after.out, len({line}), bv64(80) - len({line})) == slice(before.out, len({line}), bv64(80) - len({line}))")],
  modifies=["out"]))
fmt_regions = {"rec": {"size": "len(input.rec)", "monitor_size": "bv64(72)", "init": "input.rec", "access": "r"},
               "out": {"size": "len(before.out)", "monitor_size": "bv64(80)", "init": "before.out", "access": "rw", "observe_as": "after.out"}}
w("check/fmt_line/binding.toml", binding("todo.fmt_line", {"rdi": "addr(rec)", "rsi": "addr(out)"}, fmt_regions, {"n": "rax"}, link="link.toml", symbol="fmt.fmt_line"))
w("check/fmt_line/link.toml", '''schema = "mukoz.link/1"
# fmt_line may call udec through import slot 1 (call qword ptr [0xf0008]).

[[modules]]
name = "fmt"
path = "../../fmt_line.bin"
exports = { fmt_line = 0 }

[[modules]]
name = "num"
path = "../../udec.bin"
exports = { udec = 0 }

[imports]
1 = "num.udec"
''')
w("check/fmt_line/suite.toml", suite("fmt_line", f'''
[generate.vars.nrec]
expr = "bv64(1)"
{REC_GEN}
[generate.vars.rec]
expr = \'\'\'{dbx("bv64(1)")}\'\'\'

[generate.vars.out]
len = "bv64(80)"
''', 800))

# 5. make_rec(rdi = dst, esi = id, rdx = text, rcx = text length) : writes one 72-byte record (done 0)
newrec = (f"concat(le_bytes(input.id), concat(hex\"00\", concat(le_bytes(extract(len(input.text), 7, 0)), concat(hex\"0000\", "
          f"slice(concat(input.text, {Z64}), bv64(0), bv64(64))))))")
w("check/make_rec/contract.toml", contract("todo.make_rec", {"id": "bv32", "text": '{ type = "bytes", max_len = 64 }', "tl": "bv8"}, {"dst": '{ type = "bytes", max_len = 72 }'}, {},
  [("text_1_64", "ugt(len(input.text), bv64(0)) and len(before.dst) == bv64(72)")],
  [("record", f"after.dst == {newrec}")], modifies=["dst"]))
w("check/make_rec/binding.toml", binding("todo.make_rec", {"rdi": "addr(dst)", "rsi": "input.id", "rdx": "addr(text)", "rcx": "len(input.text)"},
  {"dst": {"size": "len(before.dst)", "monitor_size": "bv64(72)", "init": "before.dst", "access": "rw", "observe_as": "after.dst"},
   "text": {"size": "len(input.text)", "init": "input.text", "access": "r"}}, {}))
w("check/make_rec/suite.toml", suite("make_rec", '''
[generate.vars.tl]
max = "bv8(63)"

[generate.vars.text]
len = "bv64(1) + zext(input.tl, 64)"
bytes = "ascii"

[generate.vars.dst]
len = "bv64(72)"
''', 600))

# 6. del_rec(rdi = db, rsi = n, rdx = idx) -> rax = n - 1 ; record idx removed, later ones moved down
dbs = "before.db"
w("check/del_rec/contract.toml", contract("todo.del_rec", {"n": "bv64", "idx": "bv64", "pick": "bv64", **REC_PARAMS}, {"db": '{ type = "bytes", max_len = 3600 }'}, {"k": "bv64"},
  [("valid", f"ugt(input.n, bv64(0)) and ule(input.n, bv64(50)) and ult(input.idx, input.n) and len({dbs}) == input.n * bv64({R})")],
  [("removed", f"result.k == input.n - bv64(1) and slice(after.db, bv64(0), (input.n - bv64(1)) * bv64({R})) == (join i in bv64(0)..input.n: ite(i == input.idx, b\"\", {rec(dbs, 'i')}))")],
  modifies=["db"]))
w("check/del_rec/binding.toml", binding("todo.del_rec", {"rdi": "addr(db)", "rsi": "input.n", "rdx": "input.idx"},
  {"db": {"size": "len(before.db)", "monitor_size": f"input.n * bv64({R})", "init": "before.db", "access": "rw", "observe_as": "after.db"}}, {"k": "rax"}))
w("check/del_rec/suite.toml", suite("del_rec", f'''
[generate.vars.nrec]
max = "bv64(49)"
{REC_GEN}
[generate.vars.db]
expr = \'\'\'{dbx("input.nrec + bv64(1)")}\'\'\'

[generate.vars.n]
expr = "udiv(len(before.db), bv64({R}))"

[generate.vars.idx]
expr = "urem(input.pick, input.n)"
''', 600))

# 7. clear_done(rdi = db, rsi = n) -> rax = number of records kept (done 0), moved to the front in order
keep = f"(join i in bv64(0)..input.n: ite({flag(dbs, 'i')} == bv8(1), b\"\", {rec(dbs, 'i')}))"
kept = f"(count i in bv64(0)..input.n: {flag(dbs, 'i')} == bv8(0))"
w("check/clear_done/contract.toml", contract("todo.clear_done", {"n": "bv64", **REC_PARAMS}, {"db": '{ type = "bytes", max_len = 3600 }'}, {"k": "bv64"},
  [("valid", f"ule(input.n, bv64(50)) and len({dbs}) == input.n * bv64({R})")],
  [("kept", f"result.k == {kept} and slice(after.db, bv64(0), {kept} * bv64({R})) == {keep}")], modifies=["db"]))
w("check/clear_done/binding.toml", binding("todo.clear_done", {"rdi": "addr(db)", "rsi": "input.n"},
  {"db": {"size": "len(before.db)", "monitor_size": f"input.n * bv64({R})", "init": "before.db", "access": "rw", "observe_as": "after.db"}}, {"k": "rax"}))
w("check/clear_done/suite.toml", suite("clear_done", f'''
[generate.vars.nrec]
max = "bv64(50)"
{REC_GEN}
[generate.vars.db]
expr = \'\'\'{dbx("input.nrec")}\'\'\'

[generate.vars.n]
expr = "udiv(len(before.db), bv64({R}))"
''', 600))

# Standalone links for the routines that take no import (plain raw_offset bindings above) need none.
# Main program: the whole process, linked.
mods = "".join(f'\n[[modules]]\nname = "{m}"\npath = "{m}.bin"\nexports = {{ {m} = 0 }}\n' for m in MODS)
w("link.toml", f'''schema = "mukoz.link/1"
# Module i is mapped at 0x100000 + i * 0x100000 (main first). Import slot k holds the address of
# the k-th routine at 0xf0000 + 8 * k.

[[modules]]
name = "main"
path = "main.bin"
exports = {{ start = 0 }}
{mods}
[imports]
''' + "".join(f'{k} = "{m}.{m}"\n' for k, m in enumerate(MODS)) + "\n[monitors]\n" + "".join(f'"{m}.{m}" = "check/{m}/suite.toml"\n' for m in MODS))
b = open(os.path.join(HERE, "..", "todo", "binding.toml")).read()
b = b.replace('target = "x86_64/raw/sysv-x86_64/linux"', 'target = "x86_64/raw/sysv-x86_64/linux"\nlink = "../../link.toml"')
b = b.replace('[entry]\nkind = "raw_offset"\noffset = 0', '[entry]\nkind = "symbol"\nsymbol = "main.start"')
w("check/main/binding.toml", b)
shutil.copy(os.path.join(HERE, "..", "todo", "contract.toml"), os.path.join(OUT, "check/main/contract.toml"))
sm = open(os.path.join(HERE, "..", "todo", "suite.toml")).read()
w("check/main/suite.toml", sm)
