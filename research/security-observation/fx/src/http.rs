// Minimal blocking HTTP/1.0 server shared by the Rust pilot fixtures (WP1, DESIGN.md 6.1).
// One request per connection, handled in order on one thread; listens on 0.0.0.0:8000.
// The path and the query values are percent-decoded once; header names are lowercased.
#![allow(dead_code)]
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

pub struct Req {
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub fn resp(status: u16, body: impl Into<Vec<u8>>) -> Resp {
    Resp { status, headers: Vec::new(), body: body.into() }
}

pub fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        502 => "Bad Gateway",
        _ => "Status",
    }
}

fn parse(head: &str) -> Option<Req> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    if first.next()? != "GET" {
        return None;
    }
    let target = first.next()?;
    let (raw_path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let mut query = HashMap::new();
    for kv in raw_query.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        query.insert(decode(k)?, decode(v)?);
    }
    let mut headers = HashMap::new();
    for l in lines.filter(|l| !l.is_empty()) {
        let (k, v) = l.split_once(':')?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    Some(Req { path: decode(raw_path)?, query, headers })
}

fn one(mut s: TcpStream, handler: &impl Fn(&Req) -> Resp) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = s.read(&mut chunk)?;
        if n == 0 || buf.len() > 8192 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let r = match std::str::from_utf8(&buf[..end]).ok().and_then(parse) {
        Some(req) => handler(&req),
        None => resp(400, "bad request\n"),
    };
    let mut out = format!("HTTP/1.0 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n", r.status, reason(r.status), r.body.len());
    for (k, v) in &r.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    s.write_all(out.as_bytes())?;
    s.write_all(&r.body)
}

pub fn serve(handler: impl Fn(&Req) -> Resp) -> ! {
    let l = TcpListener::bind("0.0.0.0:8000").expect("bind 0.0.0.0:8000");
    loop {
        if let Ok((s, _)) = l.accept() {
            let _ = one(s, &handler);
        }
    }
}
