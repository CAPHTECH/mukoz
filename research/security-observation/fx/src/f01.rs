// F01 (DESIGN.md 6.2): GET /files/<name> serves <fx>/docs/public/<name>.
// V joins the name onto the root as given; R canonicalizes and requires the public root;
// H canonicalizes and accepts the public and shared roots (both readable by policy).
// Build: rustc -O --cfg 'variant="v"' f01.rs   (variant v, r or h)
mod http;
use std::fs;
use std::path::Path;

fn main() {
    let fx = std::env::args().nth(1).expect("usage: f01 <fixture-root>");
    let public = Path::new(&fx).join("docs/public");
    #[cfg(variant = "h")]
    let roots = [public.clone(), Path::new(&fx).join("docs/shared")];
    #[cfg(variant = "r")]
    let roots = [public.clone()];
    http::serve(move |req| {
        let Some(name) = req.path.strip_prefix("/files/") else { return http::resp(404, "no route\n") };
        let p = public.join(name);
        #[cfg(not(variant = "v"))]
        let p = match fs::canonicalize(&p) {
            Ok(c) if roots.iter().any(|r| c.starts_with(r)) => c,
            Ok(_) => return http::resp(403, "forbidden\n"),
            Err(_) => return http::resp(404, "not found\n"),
        };
        match fs::read(&p) {
            Ok(b) => http::resp(200, b),
            Err(_) => http::resp(404, "not found\n"),
        }
    })
}
