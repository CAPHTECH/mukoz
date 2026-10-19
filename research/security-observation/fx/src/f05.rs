// F05 (DESIGN.md 6.2): GET /notify?to=<ip:port>&msg=<text> sends msg to the webhook <to>.
// Only 10.77.0.1:8081 is allowed (403 otherwise; 400 if <to> is not ip:port). Connects are blocking.
// V connects to <to> as a reachability probe before checking it; R checks first;
// H checks first and reports each denial to the allowed receiver.
// Build: rustc -O --cfg 'variant="v"' f05.rs   (variant v, r or h)
mod http;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};

const ALLOWED: &str = "10.77.0.1:8081";

fn main() {
    let allowed: SocketAddr = ALLOWED.parse().unwrap();
    http::serve(move |req| {
        if req.path != "/notify" {
            return http::resp(404, "no route\n");
        }
        let (Some(to), Some(msg)) = (req.query.get("to"), req.query.get("msg")) else { return http::resp(400, "to and msg\n") };
        let Ok(addr) = to.parse::<SocketAddr>() else { return http::resp(400, "to must be ip:port\n") };
        #[cfg(variant = "v")]
        let conn = TcpStream::connect(addr);
        if addr != allowed {
            #[cfg(variant = "h")]
            if let Ok(mut s) = TcpStream::connect(allowed) {
                let _ = s.write_all(format!("denied {to}\n").as_bytes());
            }
            return http::resp(403, "destination not allowed\n");
        }
        #[cfg(not(variant = "v"))]
        let conn = TcpStream::connect(addr);
        match conn.and_then(|mut s| s.write_all(msg.as_bytes())) {
            Ok(()) => http::resp(200, "sent\n"),
            Err(_) => http::resp(502, "delivery failed\n"),
        }
    })
}
