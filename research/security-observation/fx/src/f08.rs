// F08 (DESIGN.md 6.2): GET /tdoc/<id> with "Authorization: Bearer <token>" serves
// tenants/<tenant>/<id>.txt of the token's tenant (401 bad token, 400 bad id, 404 not the caller's).
// V finds the document in whichever tenant has it (no owner check); R looks only under the caller's
// tenant; H looks the owner up in the cross-tenant index first and serves only the caller's own.
// Build: rustc -O --cfg 'variant="v"' f08.rs   (variant v, r or h)
mod http;
use std::fs;
use std::path::Path;

fn lookup(file: &Path, key: &str) -> Option<String> {
    fs::read_to_string(file).ok()?.lines().find_map(|l| {
        let (k, v) = l.split_once(' ')?;
        (k == key).then(|| v.to_string())
    })
}

fn main() {
    let fx = std::env::args().nth(1).expect("usage: f08 <fixture-root>");
    let tenants = Path::new(&fx).join("tenants");
    http::serve(move |req| {
        let Some(id) = req.path.strip_prefix("/tdoc/") else { return http::resp(404, "no route\n") };
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
            return http::resp(400, "bad id\n");
        }
        let token = req.headers.get("authorization").and_then(|v| v.strip_prefix("Bearer "));
        let Some(tenant) = token.and_then(|t| lookup(&tenants.join("tokens.txt"), t)) else {
            return http::resp(401, "unauthorized\n");
        };
        let file = format!("{id}.txt");
        #[cfg(variant = "v")]
        {
            let _ = &tenant;
            let mut dirs: Vec<_> = fs::read_dir(&tenants).into_iter().flatten().flatten().map(|e| e.path()).collect();
            dirs.sort();
            for d in dirs {
                if let Ok(b) = fs::read(d.join(&file)) {
                    return http::resp(200, b);
                }
            }
            http::resp(404, "not found\n")
        }
        #[cfg(not(variant = "v"))]
        {
            #[cfg(variant = "h")]
            if lookup(&tenants.join("index.txt"), id).as_deref() != Some(tenant.as_str()) {
                return http::resp(404, "not found\n");
            }
            match fs::read(tenants.join(&tenant).join(&file)) {
                Ok(b) => http::resp(200, b),
                Err(_) => http::resp(404, "not found\n"),
            }
        }
    })
}
