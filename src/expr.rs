//! Typed expression language (docs/04 4.3).
//!
//! Text is parsed into an AST, type-checked against declared variable types,
//! and evaluated over concrete values. Evaluation never touches the host.

use std::collections::BTreeMap;
use std::fmt;

pub const MAX_DEPTH: usize = 64;
pub const MAX_NODES: usize = 8192;
pub const MAX_FORALL: u64 = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Ty {
    Bool,
    Bv(u8),
    Bytes,
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Ty::Bool => write!(f, "bool"),
            Ty::Bv(w) => write!(f, "bv{w}"),
            Ty::Bytes => write!(f, "bytes"),
        }
    }
}

impl Ty {
    pub fn parse(s: &str) -> Option<Ty> {
        match s {
            "bool" => Some(Ty::Bool),
            "bv8" => Some(Ty::Bv(8)),
            "bv16" => Some(Ty::Bv(16)),
            "bv32" => Some(Ty::Bv(32)),
            "bv64" => Some(Ty::Bv(64)),
            "bytes" => Some(Ty::Bytes),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Bv(u8, u64),
    Bytes(Vec<u8>),
}

pub fn mask(w: u8) -> u64 {
    if w >= 64 { u64::MAX } else { (1u64 << w) - 1 }
}

fn sign_extend(w: u8, v: u64) -> i64 {
    if w >= 64 {
        v as i64
    } else {
        let shift = 64 - w as u32;
        ((v << shift) as i64) >> shift
    }
}

impl Value {
    pub fn ty(&self) -> Ty {
        match self {
            Value::Bool(_) => Ty::Bool,
            Value::Bv(w, _) => Ty::Bv(*w),
            Value::Bytes(_) => Ty::Bytes,
        }
    }
    pub fn bv(w: u8, v: u64) -> Value {
        Value::Bv(w, v & mask(w))
    }
    /// Canonical JSON form: bv as fixed-width lowercase hex, bytes as hex.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Bv(w, v) => {
                let digits = (*w as usize).div_ceil(4);
                serde_json::Value::String(format!("0x{:0width$x}", v, width = digits))
            }
            Value::Bytes(b) => serde_json::json!({ "hex": hex(b), "len": b.len() }),
        }
    }
    pub fn from_json(ty: Ty, j: &serde_json::Value) -> Option<Value> {
        match ty {
            Ty::Bool => j.as_bool().map(Value::Bool),
            Ty::Bv(w) => {
                let s = j.as_str()?;
                let v = u64::from_str_radix(s.strip_prefix("0x")?, 16).ok()?;
                if v & !mask(w) != 0 {
                    return None;
                }
                Some(Value::Bv(w, v))
            }
            Ty::Bytes => {
                let s = j.get("hex")?.as_str()?;
                unhex(s).map(Value::Bytes)
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Bv(w, v) => {
                let digits = (*w as usize).div_ceil(4);
                write!(f, "bv{w}(0x{:0width$x})", v, width = digits)
            }
            Value::Bytes(b) => {
                if b.len() <= 64 {
                    write!(f, "hex\"{}\"", hex(b))
                } else {
                    write!(f, "hex\"{}…\"(len {})", hex(&b[..64]), b.len())
                }
            }
        }
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

// ---------------------------------------------------------------- AST

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Path(Vec<String>),
    Lit(Value),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    /// Integer literal; only valid as width/index argument of zext/sext/extract.
    Int(u64),
    Index(Box<Expr>, Box<Expr>),
    Forall(String, Box<Expr>, Box<Expr>, Box<Expr>),
    /// Number of i in [lo, hi) for which the body holds (bv64).
    Count(String, Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Not,
    BitNot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    BitOr,
    BitXor,
    BitAnd,
    Add,
    Sub,
    Mul,
}

impl BinOp {
    fn sym(self) -> &'static str {
        match self {
            BinOp::Or => "or",
            BinOp::And => "and",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::BitAnd => "&",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Expr::Path(p) => write!(f, "{}", p.join(".")),
            Expr::Lit(v) => match v {
                Value::Bytes(b) => write!(f, "hex\"{}\"", hex(b)),
                _ => write!(f, "{v}"),
            },
            Expr::Int(n) => write!(f, "{n}"),
            Expr::Unary(UnOp::Not, e) => write!(f, "not {e}"),
            Expr::Unary(UnOp::BitNot, e) => write!(f, "~{e}"),
            Expr::Binary(op, a, b) => write!(f, "({a} {} {b})", op.sym()),
            Expr::Call(n, args) => {
                write!(f, "{n}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
            Expr::Index(b, i) => write!(f, "{b}[{i}]"),
            Expr::Forall(v, lo, hi, body) => write!(f, "(forall {v} in {lo}..{hi}: {body})"),
            Expr::Count(v, lo, hi, body) => write!(f, "(count {v} in {lo}..{hi}: {body})"),
        }
    }
}

// ---------------------------------------------------------------- lexer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Int(u64),
    Bytes(Vec<u8>),
    Sym(&'static str),
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == 'b' && cs.get(i + 1) == Some(&'"') {
            i += 2;
            let mut bytes = Vec::new();
            loop {
                let Some(&c) = cs.get(i) else { return Err("unterminated b\"\" literal".into()) };
                i += 1;
                match c {
                    '"' => break,
                    '\\' => {
                        let Some(&e) = cs.get(i) else { return Err("bad escape".into()) };
                        i += 1;
                        match e {
                            'n' => bytes.push(b'\n'),
                            't' => bytes.push(b'\t'),
                            'r' => bytes.push(b'\r'),
                            '0' => bytes.push(0),
                            '\\' => bytes.push(b'\\'),
                            '"' => bytes.push(b'"'),
                            'x' => {
                                let h: String = cs.get(i..i + 2).ok_or("bad \\x escape")?.iter().collect();
                                i += 2;
                                bytes.push(u8::from_str_radix(&h, 16).map_err(|_| "bad \\x escape")?);
                            }
                            _ => return Err(format!("unknown escape \\{e}")),
                        }
                    }
                    c if c.is_ascii() => bytes.push(c as u8),
                    c => {
                        let mut buf = [0u8; 4];
                        bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
            }
            out.push(Tok::Bytes(bytes));
            continue;
        }
        if c == 'h' && cs.get(i..i + 4).map(|s| s.iter().collect::<String>()) == Some("hex\"".into()) {
            i += 4;
            let start = i;
            while i < cs.len() && cs[i] != '"' {
                i += 1;
            }
            if i >= cs.len() {
                return Err("unterminated hex\"\" literal".into());
            }
            let s: String = cs[start..i].iter().filter(|c| !c.is_whitespace() && **c != '_').collect();
            i += 1;
            out.push(Tok::Bytes(unhex(&s).ok_or("bad hex literal")?));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(cs[start..i].iter().collect()));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            if c == '0' && matches!(cs.get(i + 1), Some('x') | Some('X')) {
                i += 2;
                let hs = i;
                while i < cs.len() && (cs[i].is_ascii_hexdigit() || cs[i] == '_') {
                    i += 1;
                }
                let s: String = cs[hs..i].iter().filter(|c| **c != '_').collect();
                out.push(Tok::Int(u64::from_str_radix(&s, 16).map_err(|_| "bad hex integer")?));
            } else {
                while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '_') {
                    i += 1;
                }
                let s: String = cs[start..i].iter().filter(|c| **c != '_').collect();
                out.push(Tok::Int(s.parse().map_err(|_| "bad integer")?));
            }
            continue;
        }
        let two: String = cs[i..(i + 2).min(cs.len())].iter().collect();
        let sym2 = ["==", "!=", ".."].into_iter().find(|s| *s == two);
        if let Some(s) = sym2 {
            out.push(Tok::Sym(s));
            i += 2;
            continue;
        }
        let sym1 = ["(", ")", "[", "]", ",", ".", ":", "+", "-", "*", "&", "|", "^", "~"]
            .into_iter()
            .find(|s| s.starts_with(c));
        match sym1 {
            Some(s) => {
                out.push(Tok::Sym(s));
                i += 1;
            }
            None => {
                let hint = if c == '<' || c == '>' {
                    " (use ult/ule/ugt/uge/slt/sle/sgt/sge; `<` is not part of the language)"
                } else {
                    ""
                };
                return Err(format!("unexpected character `{c}`{hint}"));
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- parser

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
    nodes: usize,
}

pub fn parse(src: &str) -> Result<Expr, String> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, depth: 0, nodes: 0 };
    let e = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(format!("unexpected token {:?} after expression", p.toks[p.pos]));
    }
    Ok(e)
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn is_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Sym(x)) if *x == s)
    }
    fn is_kw(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(x)) if x == s)
    }
    fn expect_sym(&mut self, s: &str) -> Result<(), String> {
        if self.is_sym(s) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected `{s}`, found {:?}", self.peek()))
        }
    }
    fn node(&mut self, e: Expr) -> Result<Expr, String> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(format!("expression exceeds {MAX_NODES} nodes"));
        }
        Ok(e)
    }
    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!("expression exceeds depth {MAX_DEPTH}"));
        }
        Ok(())
    }
    fn expr(&mut self) -> Result<Expr, String> {
        self.enter()?;
        let r = self.or_expr();
        self.depth -= 1;
        r
    }
    fn or_expr(&mut self) -> Result<Expr, String> {
        let mut l = self.and_expr()?;
        while self.is_kw("or") {
            self.pos += 1;
            let r = self.and_expr()?;
            l = self.node(Expr::Binary(BinOp::Or, Box::new(l), Box::new(r)))?;
        }
        Ok(l)
    }
    fn and_expr(&mut self) -> Result<Expr, String> {
        let mut l = self.not_expr()?;
        while self.is_kw("and") {
            self.pos += 1;
            let r = self.not_expr()?;
            l = self.node(Expr::Binary(BinOp::And, Box::new(l), Box::new(r)))?;
        }
        Ok(l)
    }
    fn not_expr(&mut self) -> Result<Expr, String> {
        if self.is_kw("not") {
            self.pos += 1;
            self.enter()?;
            let e = self.not_expr()?;
            self.depth -= 1;
            return self.node(Expr::Unary(UnOp::Not, Box::new(e)));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> Result<Expr, String> {
        let l = self.bitor()?;
        for (s, op) in [("==", BinOp::Eq), ("!=", BinOp::Ne)] {
            if self.is_sym(s) {
                self.pos += 1;
                let r = self.bitor()?;
                return self.node(Expr::Binary(op, Box::new(l), Box::new(r)));
            }
        }
        Ok(l)
    }
    fn chain(&mut self, ops: &[(&str, BinOp)], next: fn(&mut Self) -> Result<Expr, String>) -> Result<Expr, String> {
        let mut l = next(self)?;
        'outer: loop {
            for (s, op) in ops {
                if self.is_sym(s) {
                    self.pos += 1;
                    let r = next(self)?;
                    l = self.node(Expr::Binary(*op, Box::new(l), Box::new(r)))?;
                    continue 'outer;
                }
            }
            return Ok(l);
        }
    }
    fn bitor(&mut self) -> Result<Expr, String> {
        self.chain(&[("|", BinOp::BitOr)], Self::bitxor)
    }
    fn bitxor(&mut self) -> Result<Expr, String> {
        self.chain(&[("^", BinOp::BitXor)], Self::bitand)
    }
    fn bitand(&mut self) -> Result<Expr, String> {
        self.chain(&[("&", BinOp::BitAnd)], Self::add)
    }
    fn add(&mut self) -> Result<Expr, String> {
        self.chain(&[("+", BinOp::Add), ("-", BinOp::Sub)], Self::mul)
    }
    fn mul(&mut self) -> Result<Expr, String> {
        self.chain(&[("*", BinOp::Mul)], Self::unary)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.is_sym("~") {
            self.pos += 1;
            self.enter()?;
            let e = self.unary()?;
            self.depth -= 1;
            return self.node(Expr::Unary(UnOp::BitNot, Box::new(e)));
        }
        self.postfix()
    }
    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        while self.is_sym("[") {
            self.pos += 1;
            let i = self.expr()?;
            self.expect_sym("]")?;
            e = self.node(Expr::Index(Box::new(e), Box::new(i)))?;
        }
        Ok(e)
    }
    fn primary(&mut self) -> Result<Expr, String> {
        match self.peek().cloned() {
            Some(Tok::Sym("(")) => {
                self.pos += 1;
                let e = self.expr()?;
                self.expect_sym(")")?;
                Ok(e)
            }
            Some(Tok::Bytes(b)) => {
                self.pos += 1;
                self.node(Expr::Lit(Value::Bytes(b)))
            }
            Some(Tok::Int(n)) => {
                self.pos += 1;
                self.node(Expr::Int(n))
            }
            Some(Tok::Ident(id)) => {
                self.pos += 1;
                match id.as_str() {
                    "true" => return self.node(Expr::Lit(Value::Bool(true))),
                    "false" => return self.node(Expr::Lit(Value::Bool(false))),
                    "forall" | "count" => {
                        let Some(Tok::Ident(v)) = self.peek().cloned() else {
                            return Err(format!("expected variable after {id}"));
                        };
                        self.pos += 1;
                        if !self.is_kw("in") {
                            return Err(format!("expected `in` in {id}"));
                        }
                        self.pos += 1;
                        let lo = self.bitor()?;
                        self.expect_sym("..")?;
                        let hi = self.bitor()?;
                        self.expect_sym(":")?;
                        let body = self.expr()?;
                        let e = if id == "forall" {
                            Expr::Forall(v, Box::new(lo), Box::new(hi), Box::new(body))
                        } else {
                            Expr::Count(v, Box::new(lo), Box::new(hi), Box::new(body))
                        };
                        return self.node(e);
                    }
                    _ => {}
                }
                if let Some(w) = id.strip_prefix("bv").and_then(|w| w.parse::<u8>().ok()) {
                    if self.is_sym("(") {
                        if ![8, 16, 32, 64].contains(&w) {
                            return Err(format!("unsupported width bv{w}"));
                        }
                        self.pos += 1;
                        let neg = if self.is_sym("-") {
                            self.pos += 1;
                            true
                        } else {
                            false
                        };
                        let Some(Tok::Int(n)) = self.peek().cloned() else {
                            return Err(format!("bv{w}(...) needs an integer literal"));
                        };
                        self.pos += 1;
                        self.expect_sym(")")?;
                        if !neg && n & !mask(w) != 0 {
                            return Err(format!("literal {n:#x} does not fit in bv{w}"));
                        }
                        let v = if neg { (n as i64).wrapping_neg() as u64 } else { n };
                        if neg && (n as i128) > (1i128 << (w - 1)) {
                            return Err(format!("literal -{n} does not fit in bv{w}"));
                        }
                        return self.node(Expr::Lit(Value::bv(w, v)));
                    }
                }
                if self.is_sym("(") {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !self.is_sym(")") {
                        loop {
                            args.push(self.expr()?);
                            if self.is_sym(",") {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect_sym(")")?;
                    return self.node(Expr::Call(id, args));
                }
                let mut path = vec![id];
                while self.is_sym(".") {
                    self.pos += 1;
                    match self.peek().cloned() {
                        Some(Tok::Ident(s)) => {
                            self.pos += 1;
                            path.push(s);
                        }
                        other => return Err(format!("expected name after `.`, found {other:?}")),
                    }
                }
                self.node(Expr::Path(path))
            }
            other => Err(format!("unexpected token {other:?}")),
        }
    }
}

// ---------------------------------------------------------------- types

/// Variable types visible to an expression, keyed by dotted path.
pub type TypeEnv = BTreeMap<String, Ty>;

pub struct CheckCtx<'a> {
    pub vars: &'a TypeEnv,
    /// Extra functions allowed in this context (e.g. `addr` in bindings).
    pub allow_addr: bool,
    pub regions: &'a [String],
}

pub fn typecheck(e: &Expr, cx: &CheckCtx) -> Result<Ty, String> {
    let mut locals = Vec::new();
    tc(e, cx, &mut locals)
}

fn tc(e: &Expr, cx: &CheckCtx, locals: &mut Vec<String>) -> Result<Ty, String> {
    match e {
        Expr::Path(p) => {
            let key = p.join(".");
            if p.len() == 1 && locals.contains(&p[0]) {
                return Ok(Ty::Bv(64));
            }
            cx.vars.get(&key).copied().ok_or_else(|| format!("unknown variable `{key}`"))
        }
        Expr::Lit(v) => Ok(v.ty()),
        Expr::Int(_) => Err("bare integer literal; write bvN(...)".into()),
        Expr::Unary(UnOp::Not, a) => {
            expect(tc(a, cx, locals)?, Ty::Bool, "not")?;
            Ok(Ty::Bool)
        }
        Expr::Unary(UnOp::BitNot, a) => {
            let t = tc(a, cx, locals)?;
            if !matches!(t, Ty::Bv(_)) {
                return Err(format!("`~` needs a bitvector, got {t}"));
            }
            Ok(t)
        }
        Expr::Binary(op, a, b) => {
            let ta = tc(a, cx, locals)?;
            let tb = tc(b, cx, locals)?;
            match op {
                BinOp::Or | BinOp::And => {
                    expect(ta, Ty::Bool, op.sym())?;
                    expect(tb, Ty::Bool, op.sym())?;
                    Ok(Ty::Bool)
                }
                BinOp::Eq | BinOp::Ne => {
                    if ta != tb {
                        return Err(format!("`{}` compares {ta} with {tb}; widths must match exactly", op.sym()));
                    }
                    Ok(Ty::Bool)
                }
                _ => {
                    if !matches!(ta, Ty::Bv(_)) || ta != tb {
                        return Err(format!("`{}` needs two bitvectors of the same width, got {ta} and {tb}", op.sym()));
                    }
                    Ok(ta)
                }
            }
        }
        Expr::Index(b, i) => {
            expect(tc(b, cx, locals)?, Ty::Bytes, "indexing")?;
            expect(tc(i, cx, locals)?, Ty::Bv(64), "index")?;
            Ok(Ty::Bv(8))
        }
        Expr::Forall(v, lo, hi, body) | Expr::Count(v, lo, hi, body) => {
            let what = if matches!(e, Expr::Forall(..)) { "forall" } else { "count" };
            expect(tc(lo, cx, locals)?, Ty::Bv(64), &format!("{what} bound"))?;
            expect(tc(hi, cx, locals)?, Ty::Bv(64), &format!("{what} bound"))?;
            locals.push(v.clone());
            let t = tc(body, cx, locals);
            locals.pop();
            expect(t?, Ty::Bool, &format!("{what} body"))?;
            Ok(if what == "forall" { Ty::Bool } else { Ty::Bv(64) })
        }
        Expr::Call(name, args) => tc_call(name, args, cx, locals),
    }
}

fn expect(got: Ty, want: Ty, what: &str) -> Result<(), String> {
    if got == want { Ok(()) } else { Err(format!("{what} needs {want}, got {got}")) }
}

fn int_arg(e: &Expr, what: &str) -> Result<u64, String> {
    match e {
        Expr::Int(n) => Ok(*n),
        _ => Err(format!("{what} must be an integer literal")),
    }
}

fn tc_call(name: &str, args: &[Expr], cx: &CheckCtx, locals: &mut Vec<String>) -> Result<Ty, String> {
    let n = args.len();
    let arity = |k: usize| -> Result<(), String> {
        if n == k { Ok(()) } else { Err(format!("{name} takes {k} arguments, got {n}")) }
    };
    match name {
        "ult" | "ule" | "ugt" | "uge" | "slt" | "sle" | "sgt" | "sge" => {
            arity(2)?;
            let a = tc(&args[0], cx, locals)?;
            let b = tc(&args[1], cx, locals)?;
            if !matches!(a, Ty::Bv(_)) || a != b {
                return Err(format!("{name} needs two bitvectors of the same width, got {a} and {b}"));
            }
            Ok(Ty::Bool)
        }
        "udiv" | "urem" | "sdiv" | "srem" | "shl" | "lshr" | "ashr" => {
            arity(2)?;
            let a = tc(&args[0], cx, locals)?;
            let b = tc(&args[1], cx, locals)?;
            if !matches!(a, Ty::Bv(_)) || a != b {
                return Err(format!("{name} needs two bitvectors of the same width, got {a} and {b}"));
            }
            Ok(a)
        }
        "ite" => {
            arity(3)?;
            expect(tc(&args[0], cx, locals)?, Ty::Bool, "ite condition")?;
            let a = tc(&args[1], cx, locals)?;
            let b = tc(&args[2], cx, locals)?;
            if a != b {
                return Err(format!("ite branches differ: {a} vs {b}"));
            }
            Ok(a)
        }
        "zext" | "sext" => {
            arity(2)?;
            let a = tc(&args[0], cx, locals)?;
            let Ty::Bv(w) = a else { return Err(format!("{name} needs a bitvector")) };
            let to = int_arg(&args[1], "target width")?;
            if ![8, 16, 32, 64].contains(&to) || (to as u8) < w {
                return Err(format!("{name} from bv{w} to bv{to} is not allowed"));
            }
            Ok(Ty::Bv(to as u8))
        }
        "extract" => {
            arity(3)?;
            let Ty::Bv(w) = tc(&args[0], cx, locals)? else { return Err("extract needs a bitvector".into()) };
            let hi = int_arg(&args[1], "extract hi")?;
            let lo = int_arg(&args[2], "extract lo")?;
            let width = hi.checked_sub(lo).map(|d| d + 1).unwrap_or(0);
            if hi >= w as u64 || ![8, 16, 32, 64].contains(&width) {
                return Err(format!("extract({hi}, {lo}) of bv{w} must yield bv8/16/32/64"));
            }
            Ok(Ty::Bv(width as u8))
        }
        "len" => {
            arity(1)?;
            expect(tc(&args[0], cx, locals)?, Ty::Bytes, "len")?;
            Ok(Ty::Bv(64))
        }
        "slice" => {
            arity(3)?;
            expect(tc(&args[0], cx, locals)?, Ty::Bytes, "slice")?;
            expect(tc(&args[1], cx, locals)?, Ty::Bv(64), "slice offset")?;
            expect(tc(&args[2], cx, locals)?, Ty::Bv(64), "slice length")?;
            Ok(Ty::Bytes)
        }
        "concat" => {
            arity(2)?;
            expect(tc(&args[0], cx, locals)?, Ty::Bytes, "concat")?;
            expect(tc(&args[1], cx, locals)?, Ty::Bytes, "concat")?;
            Ok(Ty::Bytes)
        }
        "le_bytes" => {
            arity(1)?;
            let t = tc(&args[0], cx, locals)?;
            if !matches!(t, Ty::Bv(_)) {
                return Err("le_bytes needs a bitvector".into());
            }
            Ok(Ty::Bytes)
        }
        "addr" if cx.allow_addr => {
            arity(1)?;
            match &args[0] {
                Expr::Path(p) if p.len() == 1 && cx.regions.contains(&p[0]) => Ok(Ty::Bv(64)),
                _ => Err(format!("addr(...) needs a region name; known regions: {:?}", cx.regions)),
            }
        }
        _ => Err(format!("unknown function `{name}`")),
    }
}

// ---------------------------------------------------------------- evaluation

pub type ValEnv = BTreeMap<String, Value>;

pub struct EvalCtx<'a> {
    pub vars: &'a ValEnv,
    pub region_addrs: &'a BTreeMap<String, u64>,
}

pub fn eval(e: &Expr, cx: &EvalCtx) -> Result<Value, String> {
    let mut locals = Vec::new();
    ev(e, cx, &mut locals, &mut None)
}

/// Evaluate and record the value of every sub-expression (for diagnostics).
pub fn eval_traced(e: &Expr, cx: &EvalCtx, limit: usize) -> (Result<Value, String>, Vec<(String, String)>) {
    let mut locals = Vec::new();
    let mut trace = Some(Vec::new());
    let r = ev(e, cx, &mut locals, &mut trace);
    let mut t = trace.unwrap_or_default();
    // Outer expressions are pushed last; show the outermost first.
    t.reverse();
    t.truncate(limit);
    (r, t)
}

type Trace = Option<Vec<(String, String)>>;

fn ev(e: &Expr, cx: &EvalCtx, locals: &mut Vec<(String, u64)>, tr: &mut Trace) -> Result<Value, String> {
    let v = ev_inner(e, cx, locals, tr)?;
    if let Some(t) = tr.as_mut() {
        if !matches!(e, Expr::Lit(_) | Expr::Int(_)) && t.len() < 4096 {
            t.push((e.to_string(), v.to_string()));
        }
    }
    Ok(v)
}

fn bvs(v: &Value) -> Result<(u8, u64), String> {
    match v {
        Value::Bv(w, x) => Ok((*w, *x)),
        _ => Err(format!("expected bitvector, got {}", v.ty())),
    }
}
fn boolv(v: &Value) -> Result<bool, String> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(format!("expected bool, got {}", v.ty())),
    }
}
fn bytesv(v: &Value) -> Result<&[u8], String> {
    match v {
        Value::Bytes(b) => Ok(b),
        _ => Err(format!("expected bytes, got {}", v.ty())),
    }
}

fn ev_inner(e: &Expr, cx: &EvalCtx, locals: &mut Vec<(String, u64)>, tr: &mut Trace) -> Result<Value, String> {
    match e {
        Expr::Path(p) => {
            if p.len() == 1 {
                if let Some((_, v)) = locals.iter().rev().find(|(n, _)| *n == p[0]) {
                    return Ok(Value::Bv(64, *v));
                }
            }
            let key = p.join(".");
            cx.vars.get(&key).cloned().ok_or_else(|| format!("variable `{key}` has no value"))
        }
        Expr::Lit(v) => Ok(v.clone()),
        Expr::Int(n) => Ok(Value::Bv(64, *n)),
        Expr::Unary(UnOp::Not, a) => Ok(Value::Bool(!boolv(&ev(a, cx, locals, tr)?)?)),
        Expr::Unary(UnOp::BitNot, a) => {
            let (w, x) = bvs(&ev(a, cx, locals, tr)?)?;
            Ok(Value::bv(w, !x))
        }
        Expr::Binary(op, a, b) => {
            // Short-circuit is not used: both sides are evaluated so that
            // evaluation errors are never hidden by the other operand.
            let va = ev(a, cx, locals, tr)?;
            let vb = ev(b, cx, locals, tr)?;
            match op {
                BinOp::Or => Ok(Value::Bool(boolv(&va)? || boolv(&vb)?)),
                BinOp::And => Ok(Value::Bool(boolv(&va)? && boolv(&vb)?)),
                BinOp::Eq => Ok(Value::Bool(va == vb)),
                BinOp::Ne => Ok(Value::Bool(va != vb)),
                _ => {
                    let (w, x) = bvs(&va)?;
                    let (_, y) = bvs(&vb)?;
                    let r = match op {
                        BinOp::BitOr => x | y,
                        BinOp::BitXor => x ^ y,
                        BinOp::BitAnd => x & y,
                        BinOp::Add => x.wrapping_add(y),
                        BinOp::Sub => x.wrapping_sub(y),
                        BinOp::Mul => x.wrapping_mul(y),
                        _ => unreachable!(),
                    };
                    Ok(Value::bv(w, r))
                }
            }
        }
        Expr::Index(b, i) => {
            let bv = ev(b, cx, locals, tr)?;
            let bytes = bytesv(&bv)?;
            let (_, i) = bvs(&ev(i, cx, locals, tr)?)?;
            bytes
                .get(i as usize)
                .map(|x| Value::Bv(8, *x as u64))
                .ok_or_else(|| format!("index {i} out of range for bytes of length {}", bytes.len()))
        }
        Expr::Forall(v, lo, hi, body) => {
            let (_, lo) = bvs(&ev(lo, cx, locals, tr)?)?;
            let (_, hi) = bvs(&ev(hi, cx, locals, tr)?)?;
            if hi > lo && hi - lo > MAX_FORALL {
                return Err(format!("forall range {lo}..{hi} exceeds {MAX_FORALL}"));
            }
            let mut i = lo;
            while i < hi {
                locals.push((v.clone(), i));
                // Do not trace inside the loop body: only the overall result matters.
                let r = ev(body, cx, locals, &mut None);
                locals.pop();
                if !boolv(&r?)? {
                    if let Some(t) = tr.as_mut() {
                        t.push((format!("first {v} where body is false"), i.to_string()));
                    }
                    return Ok(Value::Bool(false));
                }
                i += 1;
            }
            Ok(Value::Bool(true))
        }
        Expr::Count(v, lo, hi, body) => {
            let (_, lo) = bvs(&ev(lo, cx, locals, tr)?)?;
            let (_, hi) = bvs(&ev(hi, cx, locals, tr)?)?;
            if hi > lo && hi - lo > MAX_FORALL {
                return Err(format!("count range {lo}..{hi} exceeds {MAX_FORALL}"));
            }
            let mut n = 0u64;
            let mut i = lo;
            while i < hi {
                locals.push((v.clone(), i));
                let r = ev(body, cx, locals, &mut None);
                locals.pop();
                if boolv(&r?)? {
                    n += 1;
                }
                i += 1;
            }
            Ok(Value::Bv(64, n))
        }
        Expr::Call(name, args) => {
            if name == "ite" {
                // Lazy: only the selected branch is evaluated.
                let c = boolv(&ev(&args[0], cx, locals, tr)?)?;
                return ev(&args[if c { 1 } else { 2 }], cx, locals, tr);
            }
            if name == "addr" {
                let Expr::Path(p) = &args[0] else { return Err("addr needs a region".into()) };
                return cx
                    .region_addrs
                    .get(&p[0])
                    .map(|a| Value::Bv(64, *a))
                    .ok_or_else(|| format!("region `{}` has no address", p[0]));
            }
            if matches!(name.as_str(), "zext" | "sext") {
                let (w, x) = bvs(&ev(&args[0], cx, locals, tr)?)?;
                let to = int_arg(&args[1], "width")? as u8;
                let r = if name == "zext" { x } else { sign_extend(w, x) as u64 };
                return Ok(Value::bv(to, r));
            }
            if name == "extract" {
                let (_, x) = bvs(&ev(&args[0], cx, locals, tr)?)?;
                let hi = int_arg(&args[1], "hi")?;
                let lo = int_arg(&args[2], "lo")?;
                let w = (hi - lo + 1) as u8;
                return Ok(Value::bv(w, x >> lo));
            }
            let vals: Vec<Value> = args.iter().map(|a| ev(a, cx, locals, tr)).collect::<Result<_, _>>()?;
            call(name, &vals)
        }
    }
}

fn call(name: &str, a: &[Value]) -> Result<Value, String> {
    match name {
        "ult" | "ule" | "ugt" | "uge" | "slt" | "sle" | "sgt" | "sge" => {
            let (w, x) = bvs(&a[0])?;
            let (_, y) = bvs(&a[1])?;
            let (sx, sy) = (sign_extend(w, x), sign_extend(w, y));
            Ok(Value::Bool(match name {
                "ult" => x < y,
                "ule" => x <= y,
                "ugt" => x > y,
                "uge" => x >= y,
                "slt" => sx < sy,
                "sle" => sx <= sy,
                "sgt" => sx > sy,
                _ => sx >= sy,
            }))
        }
        "udiv" | "urem" | "sdiv" | "srem" => {
            let (w, x) = bvs(&a[0])?;
            let (_, y) = bvs(&a[1])?;
            if y == 0 {
                return Err(format!("{name} by zero"));
            }
            let (sx, sy) = (sign_extend(w, x), sign_extend(w, y));
            let r = match name {
                "udiv" => x / y,
                "urem" => x % y,
                "sdiv" => sx.wrapping_div(sy) as u64,
                _ => sx.wrapping_rem(sy) as u64,
            };
            Ok(Value::bv(w, r))
        }
        "shl" | "lshr" | "ashr" => {
            let (w, x) = bvs(&a[0])?;
            let (_, s) = bvs(&a[1])?;
            let r = if s >= w as u64 {
                match name {
                    "ashr" if sign_extend(w, x) < 0 => u64::MAX,
                    _ => 0,
                }
            } else {
                match name {
                    "shl" => x << s,
                    "lshr" => x >> s,
                    _ => (sign_extend(w, x) >> s) as u64,
                }
            };
            Ok(Value::bv(w, r))
        }
        "ite" => Ok(if boolv(&a[0])? { a[1].clone() } else { a[2].clone() }),
        "len" => Ok(Value::Bv(64, bytesv(&a[0])?.len() as u64)),
        "slice" => {
            let b = bytesv(&a[0])?;
            let (_, off) = bvs(&a[1])?;
            let (_, n) = bvs(&a[2])?;
            let end = off.checked_add(n).ok_or("slice range overflows")?;
            if end > b.len() as u64 {
                return Err(format!("slice {off}+{n} out of range for length {}", b.len()));
            }
            Ok(Value::Bytes(b[off as usize..end as usize].to_vec()))
        }
        "concat" => {
            let mut v = bytesv(&a[0])?.to_vec();
            v.extend_from_slice(bytesv(&a[1])?);
            Ok(Value::Bytes(v))
        }
        "le_bytes" => {
            let (w, x) = bvs(&a[0])?;
            Ok(Value::Bytes(x.to_le_bytes()[..(w / 8) as usize].to_vec()))
        }
        _ => Err(format!("unknown function `{name}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev_str(src: &str, vars: &[(&str, Value)]) -> Value {
        let e = parse(src).unwrap();
        let env: ValEnv = vars.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let tenv: TypeEnv = vars.iter().map(|(k, v)| (k.to_string(), v.ty())).collect();
        typecheck(&e, &CheckCtx { vars: &tenv, allow_addr: false, regions: &[] }).unwrap();
        eval(&e, &EvalCtx { vars: &env, region_addrs: &BTreeMap::new() }).unwrap()
    }

    // Expected values below are fixed by hand, not computed by the evaluator.
    #[test]
    fn known_answers_bv() {
        let a = Value::Bv(64, u64::MAX);
        let b = Value::Bv(64, 2);
        assert_eq!(ev_str("input.a + input.b", &[("input.a", a.clone()), ("input.b", b.clone())]), Value::Bv(64, 1));
        assert_eq!(ev_str("input.b - input.a", &[("input.a", a.clone()), ("input.b", b.clone())]), Value::Bv(64, 3));
        assert_eq!(ev_str("slt(input.a, input.b)", &[("input.a", a.clone()), ("input.b", b.clone())]), Value::Bool(true));
        assert_eq!(ev_str("ult(input.a, input.b)", &[("input.a", a.clone()), ("input.b", b.clone())]), Value::Bool(false));
        assert_eq!(ev_str("sext(bv8(0x80), 64)", &[]), Value::Bv(64, 0xffff_ffff_ffff_ff80));
        assert_eq!(ev_str("zext(bv8(0x80), 64)", &[]), Value::Bv(64, 0x80));
        assert_eq!(ev_str("extract(bv64(0x1122334455667788), 15, 8)", &[]), Value::Bv(8, 0x77));
        assert_eq!(ev_str("ashr(bv32(0x80000000), bv32(31))", &[]), Value::Bv(32, 0xffff_ffff));
        assert_eq!(ev_str("lshr(bv32(0x80000000), bv32(31))", &[]), Value::Bv(32, 1));
        assert_eq!(ev_str("bv16(-1)", &[]), Value::Bv(16, 0xffff));
        assert_eq!(ev_str("sdiv(bv8(-7), bv8(2))", &[]), Value::Bv(8, 0xfd));
    }

    #[test]
    fn known_answers_bytes() {
        let s = Value::Bytes(b"abc".to_vec());
        assert_eq!(ev_str("len(x)", &[("x", s.clone())]), Value::Bv(64, 3));
        assert_eq!(ev_str("x[bv64(1)]", &[("x", s.clone())]), Value::Bv(8, b'b' as u64));
        assert_eq!(ev_str("x == b\"abc\"", &[("x", s.clone())]), Value::Bool(true));
        assert_eq!(ev_str("x == hex\"616263\"", &[("x", s.clone())]), Value::Bool(true));
        assert_eq!(ev_str("forall i in bv64(0)..len(x): ult(x[i], bv8(0x64))", &[("x", s.clone())]), Value::Bool(true));
        assert_eq!(ev_str("forall i in bv64(0)..len(x): ult(x[i], bv8(0x63))", &[("x", s.clone())]), Value::Bool(false));
        assert_eq!(ev_str("le_bytes(bv32(0x01020304)) == hex\"04030201\"", &[]), Value::Bool(true));
    }

    #[test]
    fn known_answers_count_and_lazy_ite() {
        let x = Value::Bv(64, 0xf0f0);
        assert_eq!(ev_str("count i in bv64(0)..bv64(64): (lshr(x, i) & bv64(1)) == bv64(1)", &[("x", x)]), Value::Bv(64, 8));
        let s = Value::Bytes(b"abca".to_vec());
        assert_eq!(ev_str("count i in bv64(0)..len(s): s[i] == bv8(0x61)", &[("s", s)]), Value::Bv(64, 2));
        let z = Value::Bv(64, 0);
        assert_eq!(ev_str("ite(a == bv64(0), true, udiv(bv64(5), a) == bv64(1))", &[("a", z)]), Value::Bool(true));
    }

    #[test]
    fn rejects_implicit_width_and_lt() {
        let tenv: TypeEnv = [("a".to_string(), Ty::Bv(64)), ("b".to_string(), Ty::Bv(32))].into();
        let cx = CheckCtx { vars: &tenv, allow_addr: false, regions: &[] };
        assert!(typecheck(&parse("a + b").unwrap(), &cx).is_err());
        assert!(typecheck(&parse("a == b").unwrap(), &cx).is_err());
        assert!(parse("a < a").is_err());
        assert!(parse("bv8(256)").is_err());
        assert!(typecheck(&parse("a + 1").unwrap(), &cx).is_err());
    }

    #[test]
    fn precedence_not_binds_looser_than_eq() {
        let e = parse("not a == a").unwrap();
        assert!(matches!(e, Expr::Unary(UnOp::Not, _)));
    }
}
