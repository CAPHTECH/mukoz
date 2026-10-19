// F03 (DESIGN.md 6.2): GET /doc/<name> serves <fx>/docs/<name> unless docs/.acl lists the name (403).
// V opens the file and sniffs its first 512 bytes before consulting the ACL; R consults the ACL first;
// H consults the ACL first and, for a denied name, takes an O_PATH handle (not readable) to put
// the file's mtime in the 403.
// Build: rustc -O --cfg 'variant="v"' f03.rs   (variant v, r or h)
mod http;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

fn text_like(head: &[u8]) -> bool {
    head.iter().all(|&c| c == b'\n' || c == b'\t' || (0x20..0x7f).contains(&c))
}

fn main() {
    let fx = std::env::args().nth(1).expect("usage: f03 <fixture-root>");
    let docs = Path::new(&fx).join("docs");
    http::serve(move |req| {
        let Some(name) = req.path.strip_prefix("/doc/") else { return http::resp(404, "no route\n") };
        if name.is_empty() || name.starts_with('.') || !name.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)) {
            return http::resp(400, "bad name\n");
        }
        let p = docs.join(name);
        #[cfg(variant = "v")]
        let (mut f, mut body) = match File::open(&p) {
            Ok(mut f) => {
                let mut head = Vec::new();
                if (&mut f).take(512).read_to_end(&mut head).is_err() {
                    return http::resp(404, "not found\n");
                }
                (f, head)
            }
            Err(_) => return http::resp(404, "not found\n"),
        };
        // Fail closed: an unreadable ACL denies everything.
        let denied = fs::read_to_string(docs.join(".acl")).map(|a| a.lines().any(|l| l == name)).unwrap_or(true);
        if denied {
            #[allow(unused_mut)]
            let mut r = http::resp(403, "forbidden\n");
            #[cfg(variant = "h")]
            {
                use std::os::unix::fs::OpenOptionsExt;
                const O_PATH: i32 = 0o10000000;
                let h = fs::OpenOptions::new().read(true).custom_flags(O_PATH).open(&p);
                if let Ok(Ok(m)) = h.map(|f| f.metadata()) {
                    use std::os::unix::fs::MetadataExt;
                    r.headers.push(("X-Mtime".into(), m.mtime().to_string()));
                }
            }
            return r;
        }
        #[cfg(not(variant = "v"))]
        let (mut f, mut body) = match File::open(&p) {
            Ok(f) => (f, Vec::new()),
            Err(_) => return http::resp(404, "not found\n"),
        };
        if f.read_to_end(&mut body).is_err() {
            return http::resp(404, "not found\n");
        }
        let mut r = http::resp(200, body);
        let ctype = if text_like(&r.body[..r.body.len().min(512)]) { "text/plain" } else { "application/octet-stream" };
        r.headers.push(("Content-Type".into(), ctype.into()));
        r
    })
}
