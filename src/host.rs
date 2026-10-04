//! Host probe and isolation (docs/02 2.5, docs/08 8.3–8.4). Linux only; other hosts report
//! every capability as unconfirmed. A capability counts only after it was tried here and worked.

use serde_json::{Value, json};
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const PROBE_VERSION: &str = "mukoz-probe/1";

/// What to apply to a child before running the payload.
#[derive(Clone, Debug, Default)]
pub struct Sandbox {
    /// New user + mount namespace; needed for everything below except rlimits.
    pub user_ns: bool,
    pub net_ns: bool,
    /// New PID namespace: the payload runs as its init, so every descendant dies with it.
    pub pid_ns: bool,
    /// chroot into this directory (requires user_ns).
    pub root: Option<PathBuf>,
    pub cpu_s: Option<u64>,
    pub as_bytes: Option<u64>,
    pub fsize: Option<u64>,
    pub nproc: Option<u64>,
    pub nofile: Option<u64>,
}

impl Sandbox {
    /// The isolation the trial zone requires (docs/08 8.4), with a root directory.
    pub fn trial(root: Option<PathBuf>, cpu_s: u64) -> Sandbox {
        Sandbox { user_ns: true, net_ns: true, pid_ns: true, root, cpu_s: Some(cpu_s), as_bytes: Some(1 << 30), fsize: Some(16 << 20), nproc: Some(64), nofile: Some(64) }
    }
    pub fn applied(&self) -> Vec<&'static str> {
        let mut v = vec!["process_isolation", "credential_isolation"];
        if self.net_ns {
            v.push("network_restriction");
        }
        if self.root.is_some() {
            v.push("filesystem_restriction");
        }
        if self.pid_ns {
            v.push("descendant_process_control");
        }
        if self.cpu_s.is_some() || self.as_bytes.is_some() || self.fsize.is_some() {
            v.push("resource_limits");
        }
        v
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
    TimedOut,
}

fn write_file(path: &str, s: &str) -> bool {
    let Ok(c) = CString::new(path) else { return false };
    unsafe {
        let fd = libc::open(c.as_ptr(), libc::O_WRONLY);
        if fd < 0 {
            return false;
        }
        let ok = libc::write(fd, s.as_ptr() as *const _, s.len()) == s.len() as isize;
        libc::close(fd);
        ok
    }
}

fn setrl(res: libc::__rlimit_resource_t, v: Option<u64>) -> bool {
    match v {
        None => true,
        Some(v) => unsafe { libc::setrlimit(res, &libc::rlimit { rlim_cur: v, rlim_max: v }) == 0 },
    }
}

/// Close every fd from 3 up except those listed (close_range, Linux 5.9+; a loop otherwise).
fn close_fds_except(keep: &[i32]) {
    let mut k: Vec<u32> = keep.iter().filter(|&&f| f >= 3).map(|&f| f as u32).collect();
    k.sort();
    k.dedup();
    let mut lo = 3u32;
    for f in k.into_iter().chain(std::iter::once(u32::MAX)) {
        if f > lo {
            let hi = f - 1;
            let r = unsafe { libc::syscall(libc::SYS_close_range, lo, hi, 0) };
            if r != 0 {
                let max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) }.clamp(64, 65536) as u32;
                for fd in lo..=hi.min(max) {
                    unsafe { libc::close(fd as i32) };
                }
            }
        }
        lo = f.saturating_add(1);
    }
}

/// Enter the sandbox in the current (already forked) process. Returns false if a step failed;
/// the caller must then exit without running the payload. `keep` lists fds to leave open.
fn enter(sb: &Sandbox, keep: &[i32]) -> bool {
    unsafe {
        let uid = libc::getuid();
        let gid = libc::getgid();
        if sb.user_ns {
            let mut flags = libc::CLONE_NEWUSER | libc::CLONE_NEWNS;
            if sb.net_ns {
                flags |= libc::CLONE_NEWNET;
            }
            if libc::unshare(flags) != 0 {
                return false;
            }
            if !write_file("/proc/self/setgroups", "deny") || !write_file("/proc/self/uid_map", &format!("0 {uid} 1")) || !write_file("/proc/self/gid_map", &format!("0 {gid} 1")) {
                return false;
            }
        }
        if let Some(r) = &sb.root {
            if !sb.user_ns {
                return false;
            }
            let Ok(c) = CString::new(r.as_os_str().as_encoded_bytes()) else { return false };
            if libc::chroot(c.as_ptr()) != 0 || libc::chdir(c"/".as_ptr()) != 0 {
                return false;
            }
        }
        if !(setrl(libc::RLIMIT_CPU, sb.cpu_s) && setrl(libc::RLIMIT_AS, sb.as_bytes) && setrl(libc::RLIMIT_FSIZE, sb.fsize) && setrl(libc::RLIMIT_NPROC, sb.nproc) && setrl(libc::RLIMIT_NOFILE, sb.nofile) && setrl(libc::RLIMIT_CORE, Some(0))) {
            return false;
        }
        libc::clearenv();
        close_fds_except(keep);
        true
    }
}

/// Run `payload` in a fresh child under `sb` and wait at most `wall_ms`. The payload's return
/// value is the exit code. On timeout the whole process group (and the PID namespace) is killed.
/// `keep` lists fds the payload needs (e.g. a result pipe). Exit code 125 means the sandbox
/// could not be entered (the payload did not run).
pub fn run<F: FnOnce() -> i32>(sb: &Sandbox, wall_ms: u64, keep: &[i32], payload: F) -> Exit {
    let mut st = [0i32; 2];
    unsafe {
        if libc::pipe2(st.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            return Exit::Code(125);
        }
    }
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Exit::Code(125);
    }
    if pid == 0 {
        // Child A: namespaces, then (with a PID namespace) child B runs the payload as init.
        unsafe {
            libc::setpgid(0, 0);
            libc::close(st[0]);
            let mut keep_a: Vec<i32> = keep.to_vec();
            keep_a.push(st[1]);
            if sb.pid_ns {
                let lim = Sandbox { user_ns: sb.user_ns, net_ns: sb.net_ns, ..Default::default() };
                if !enter(&lim, &keep_a) || libc::unshare(libc::CLONE_NEWPID) != 0 {
                    libc::_exit(125);
                }
                let b = libc::fork();
                if b < 0 {
                    libc::_exit(125);
                }
                if b == 0 {
                    libc::close(st[1]);
                    let rest = Sandbox { user_ns: false, net_ns: false, pid_ns: false, ..sb.clone() };
                    // chroot needs the user namespace entered above (same process tree).
                    if let Some(r) = &sb.root {
                        let Ok(c) = CString::new(r.as_os_str().as_encoded_bytes()) else { libc::_exit(125) };
                        if libc::chroot(c.as_ptr()) != 0 || libc::chdir(c"/".as_ptr()) != 0 {
                            libc::_exit(125);
                        }
                    }
                    let rest = Sandbox { root: None, ..rest };
                    if !enter(&rest, keep) {
                        libc::_exit(125);
                    }
                    let r = payload();
                    libc::_exit(r);
                }
                let mut status = 0;
                while libc::waitpid(b, &mut status, 0) < 0 {}
                let bytes = status.to_le_bytes();
                libc::write(st[1], bytes.as_ptr() as *const _, 4);
                libc::_exit(0);
            }
            // Without a PID namespace this process is the payload: it reports nothing through
            // the status pipe, and descendants must not inherit it (the parent would wait on them).
            libc::close(st[1]);
            if !enter(sb, keep) {
                libc::_exit(125);
            }
            let r = payload();
            libc::_exit(r);
        }
    }
    unsafe { libc::close(st[1]) };
    let deadline = Instant::now() + Duration::from_millis(wall_ms);
    let mut status = 0;
    let timed_out = loop {
        let r = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if r == pid {
            break false;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
                libc::kill(pid, libc::SIGKILL);
                while libc::waitpid(pid, &mut status, 0) < 0 {}
            }
            break true;
        }
        std::thread::sleep(Duration::from_micros(200));
    };
    let mut inner = [0u8; 4];
    let got = unsafe {
        let fl = libc::fcntl(st[0], libc::F_GETFL);
        libc::fcntl(st[0], libc::F_SETFL, fl | libc::O_NONBLOCK);
        libc::read(st[0], inner.as_mut_ptr() as *mut _, 4)
    };
    unsafe { libc::close(st[0]) };
    if timed_out {
        return Exit::TimedOut;
    }
    if got == 4 {
        status = i32::from_le_bytes(inner);
    }
    if libc::WIFEXITED(status) {
        Exit::Code(libc::WEXITSTATUS(status))
    } else if libc::WIFSIGNALED(status) {
        Exit::Signal(libc::WTERMSIG(status))
    } else {
        Exit::Code(125)
    }
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mukoz-{tag}-{}-{}", std::process::id(), rand_u64()));
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn rand_u64() -> u64 {
    let mut b = [0u8; 8];
    unsafe { libc::getrandom(b.as_mut_ptr() as *mut _, 8, 0) };
    u64::from_le_bytes(b)
}

pub fn make_trial_dir(tag: &str) -> PathBuf {
    tmpdir(tag)
}

fn cap(ok: bool, how: &str) -> Value {
    json!({ "confirmed": ok, "how": how })
}

fn probe_network() -> Value {
    let check = || unsafe {
        let s = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if s < 0 {
            return 0;
        }
        let addr = libc::sockaddr_in { sin_family: libc::AF_INET as u16, sin_port: 53u16.to_be(), sin_addr: libc::in_addr { s_addr: u32::from_be_bytes([8, 8, 8, 8]).to_be() }, sin_zero: [0; 8] };
        let r = libc::connect(s, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as u32);
        if r == 0 { 1 } else { 0 }
    };
    let on = run(&Sandbox { user_ns: true, net_ns: true, ..Default::default() }, 3000, &[], check);
    let off = run(&Sandbox { user_ns: true, ..Default::default() }, 3000, &[], check);
    controlled(on == Exit::Code(0), off == Exit::Code(1), "user+network namespace: a UDP connect to 8.8.8.8:53 failed inside it", "the same connect succeeded without the network namespace")
}

/// A capability is confirmed only when the restricted run blocked the access and an otherwise
/// identical control run without the restriction reached it (so the check itself can see it).
fn controlled(restricted_ok: bool, control_reached: bool, how: &str, control: &str) -> Value {
    json!({ "confirmed": restricted_ok && control_reached, "how": how, "control": control, "restricted_blocked": restricted_ok, "control_reached": control_reached })
}

fn probe_fs() -> Value {
    let d = tmpdir("probe-fs");
    let check = || unsafe {
        let a = libc::open(c"/etc/passwd".as_ptr(), libc::O_RDONLY);
        let b = libc::open(c"/proc/self/status".as_ptr(), libc::O_RDONLY);
        if a < 0 && b < 0 { 0 } else { 1 }
    };
    let on = run(&Sandbox { user_ns: true, root: Some(d.clone()), ..Default::default() }, 3000, &[], check);
    let off = run(&Sandbox { user_ns: true, ..Default::default() }, 3000, &[], check);
    let _ = std::fs::remove_dir_all(&d);
    controlled(on == Exit::Code(0), off == Exit::Code(1), "user+mount namespace and chroot into an empty directory: /etc/passwd and /proc were unreachable", "both were readable without the chroot")
}

fn probe_rlimits() -> Value {
    let d = tmpdir("probe-rl");
    let check = || unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        let fd = libc::open(c"/big".as_ptr(), libc::O_WRONLY | libc::O_CREAT, 0o600);
        if fd < 0 {
            return 2;
        }
        let buf = [0u8; 8192];
        let w = libc::write(fd, buf.as_ptr() as *const _, buf.len());
        let mut rl = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        libc::getrlimit(libc::RLIMIT_CPU, &mut rl);
        if w == 4096 && rl.rlim_cur == 5 { 0 } else { 1 }
    };
    let on = run(&Sandbox { user_ns: true, root: Some(d.clone()), fsize: Some(4096), cpu_s: Some(5), as_bytes: Some(1 << 30), ..Default::default() }, 3000, &[], check);
    let _ = std::fs::remove_file(d.join("big"));
    let off = run(&Sandbox { user_ns: true, root: Some(d.clone()), ..Default::default() }, 3000, &[], check);
    let _ = std::fs::remove_dir_all(&d);
    controlled(on == Exit::Code(0), off == Exit::Code(1), "setrlimit: a write past RLIMIT_FSIZE was cut at the limit and RLIMIT_CPU read back", "the same write went through whole without the limits")
}

fn probe_descendants() -> Value {
    let on = descendant_dies(true);
    let off = descendant_dies(false);
    controlled(on, !off, "PID namespace: a sleeping grandchild died when the sandbox init exited", "without the PID namespace the grandchild outlived its parent")
}

fn descendant_dies(pid_ns: bool) -> bool {
    let mut p = [0i32; 2];
    unsafe { libc::pipe(p.as_mut_ptr()) };
    let sb = Sandbox { user_ns: true, pid_ns, ..Default::default() };
    let w = p[1];
    let e = run(&sb, 3000, &[w], move || unsafe {
        let g = libc::fork();
        if g == 0 {
            libc::write(w, b"g".as_ptr() as *const _, 1);
            libc::sleep(2);
            libc::_exit(0);
        }
        if g < 0 { 1 } else { 0 }
    });
    unsafe { libc::close(p[1]) };
    // EOF arrives only when every holder of the write end is gone, i.e. the grandchild died.
    let start = Instant::now();
    let mut eof = false;
    unsafe {
        let fl = libc::fcntl(p[0], libc::F_GETFL);
        libc::fcntl(p[0], libc::F_SETFL, fl | libc::O_NONBLOCK);
        let mut b = [0u8; 1];
        while start.elapsed() < Duration::from_millis(500) {
            let r = libc::read(p[0], b.as_mut_ptr() as *mut _, 1);
            if r == 1 && std::env::var_os("MUKOZ_PROBE_DEBUG").is_some() {
                eprintln!("got byte {:?} at {:?}", b[0] as char, start.elapsed());
            }
            if r == 0 {
                eof = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        libc::close(p[0]);
    }
    if std::env::var_os("MUKOZ_PROBE_DEBUG").is_some() {
        eprintln!("descendant_dies(pid_ns={pid_ns}): exit {e:?}, eof {eof} after {:?}", start.elapsed());
    }
    e == Exit::Code(0) && eof
}

fn probe_credentials() -> Value {
    let mut p = [0i32; 2];
    unsafe { libc::pipe(p.as_mut_ptr()) };
    let extra = unsafe { libc::dup(p[0]) };
    let sb = Sandbox { user_ns: true, ..Default::default() };
    let e = run(&sb, 3000, &[], move || unsafe {
        let env_empty = std::env::vars_os().next().is_none();
        let fd_closed = libc::fcntl(extra, libc::F_GETFD) < 0;
        if env_empty && fd_closed { 0 } else { 1 }
    });
    unsafe {
        libc::close(p[0]);
        libc::close(p[1]);
        libc::close(extra);
    }
    cap(e == Exit::Code(0), "the environment was empty and an inherited descriptor was closed inside the sandbox")
}

fn probe_seccomp_strict() -> Value {
    let on = run(&Sandbox::default(), 3000, &[], || unsafe {
        if libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_STRICT as libc::c_ulong) != 0 {
            return 2;
        }
        libc::syscall(libc::SYS_getpid);
        1
    });
    let off = run(&Sandbox::default(), 3000, &[], || unsafe {
        libc::syscall(libc::SYS_getpid);
        1
    });
    controlled(on == Exit::Signal(libc::SIGKILL), off == Exit::Code(1), "seccomp strict mode: a getpid after it killed the process", "the same getpid returned normally without it")
}

fn probe_memfd_seal() -> Value {
    unsafe {
        let fd = libc::memfd_create(c"mukoz-probe".as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING);
        if fd < 0 {
            return cap(false, "memfd_create failed");
        }
        let b = b"abc";
        libc::write(fd, b.as_ptr() as *const _, 3);
        let sealed = libc::fcntl(fd, libc::F_ADD_SEALS, libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL) == 0;
        let w = libc::pwrite(fd, b.as_ptr() as *const _, 3, 0);
        libc::close(fd);
        cap(sealed && w < 0, "memfd with write/grow/shrink seals; a later write was refused")
    }
}

fn cpu_info() -> Value {
    let text = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let field = |k: &str| text.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_once(':')).map(|(_, v)| v.trim().to_string());
    let flags = field("flags").or_else(|| field("Features")).unwrap_or_default();
    json!({ "model": field("model name").or_else(|| field("CPU part")), "features_sha256": crate::spec::sha256_hex(flags.as_bytes()), "features": flags.split_whitespace().count() })
}

pub fn host_id() -> String {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    unsafe { libc::uname(&mut u) };
    let s = |a: &[libc::c_char]| unsafe { std::ffi::CStr::from_ptr(a.as_ptr()) }.to_string_lossy().into_owned();
    format!("{}-{} {} {}", std::env::consts::OS, std::env::consts::ARCH, s(&u.release), cpu_info()["features_sha256"].as_str().unwrap_or("")[..12].to_string())
}

/// Try every capability on this host (docs/02 2.5, docs/08 8.3).
pub fn probe() -> Value {
    let linux = cfg!(target_os = "linux");
    let caps = if linux {
        json!({
            "process_isolation": cap(true, "fork; the payload runs in its own process group"),
            "network_restriction": probe_network(),
            "filesystem_restriction": probe_fs(),
            "resource_limits": probe_rlimits(),
            "descendant_process_control": probe_descendants(),
            "credential_isolation": probe_credentials(),
            "artifact_immutability": probe_memfd_seal(),
            "seccomp_strict": probe_seccomp_strict(),
        })
    } else {
        json!({})
    };
    let ok = |k: &str| caps[k]["confirmed"].as_bool().unwrap_or(false);
    let host_isa = std::env::consts::ARCH;
    let trial = ["network_restriction", "filesystem_restriction", "resource_limits", "descendant_process_control", "credential_isolation"].iter().all(|k| ok(k));
    json!({
        "probe": PROBE_VERSION,
        "host_id": host_id(),
        "os": std::env::consts::OS,
        "arch": host_isa,
        "cpu": cpu_info(),
        "capabilities": caps,
        "executors": {
            "emulated": { "available": true, "engine": crate::emu::ENGINE, "isas": ["x86_64", "aarch64"], "note": "usable for an ISA once `platform qualify` passed on this host (checked at every run)" },
            "native-routine": { "available": linux && host_isa == "x86_64" && ok("seccomp_strict"), "isas": if host_isa == "x86_64" { json!(["x86_64"]) } else { json!([]) }, "observes": ["return registers", "callee-saved registers", "memory regions after return", "crashes (signals)"], "does_not_observe": ["individual memory accesses (page-granular guard only)", "attempted system calls beyond seccomp strict's kill"] },
            "native-process": { "available": linux && host_isa == "x86_64" && trial, "targets": ["x86_64/raw/sysv-x86_64/linux", "x86_64/elf/sysv-x86_64/linux"], "observes": ["stdout/stderr bytes", "exit status or signal", "declared files after exit"], "does_not_observe": ["registers", "memory accesses", "system calls"] },
            "translated-process": { "available": false, "reason": "no translation layer (qemu-user) on this host" },
        },
        "trial_zone_isolation_confirmed": trial,
    })
}

/// The stored probe for this host, refreshed when missing or from another host/probe version.
pub fn current_probe(store: &crate::store::Store) -> Value {
    if let Some(p) = store.get_host("probe") {
        if p["probe"] == PROBE_VERSION && p["host_id"] == host_id() {
            return p;
        }
    }
    let p = probe();
    let _ = store.put_host("probe", &p);
    p
}

pub fn remove_dir(p: &Path) {
    let _ = std::fs::remove_dir_all(p);
}
