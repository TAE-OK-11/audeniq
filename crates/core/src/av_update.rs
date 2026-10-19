//! FreshClam behind a loopback proxy that permits only official TLS tunnels
//! (`audeniq-admin antivirus-update`, the `antivirus-update` service).
//! Replaces deploy/update-antivirus.py, so the runtime image needs no Python.
//!
//! FreshClam/libcurl may follow a redirect to HTTP. That request reaches this
//! proxy as a plain `GET` and is refused locally, before it can open an
//! external plaintext connection. Only `CONNECT database.clamav.net:443` is
//! tunnelled; certificate verification and TLS stay end to end in FreshClam,
//! the proxy never decrypts.
use std::{
    io::{self, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    process::{Command, ExitStatus},
    sync::{
        Arc,
        atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub const MIRROR: &str = "database.clamav.net";
pub const PORT: u16 = 18080;
/// Largest request head read before refusing.
const HEAD_MAX: usize = 8 * 1024;
/// Open tunnels at a time (FreshClam uses one or two).
const MAX_TUNNELS: usize = 16;

/// Tunnel limits (tests shorten them).
#[derive(Clone, Copy)]
pub struct Limits {
    /// Bytes relayed per tunnel, both directions together.
    pub bytes: u64,
    /// A tunnel with no data in either direction this long is closed.
    pub idle: Duration,
    pub connect: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 1024 * 1024 * 1024,
            idle: Duration::from_secs(180),
            connect: Duration::from_secs(10),
        }
    }
}

/// Opens the upstream connection for an allowed tunnel.
pub type Dial = dyn Fn(&str, u16, Duration) -> io::Result<TcpStream> + Send + Sync;

pub fn dial_mirror(host: &str, port: u16, timeout: Duration) -> io::Result<TcpStream> {
    use std::net::ToSocketAddrs;
    let mut last = io::Error::from(io::ErrorKind::NotFound);
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn refuse(conn: &mut TcpStream) {
    let _ =
        conn.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = conn.shutdown(Shutdown::Both);
}

/// Reads the request head; returns (request line, bytes already read past it).
fn read_head(conn: &mut TcpStream) -> io::Result<(String, Vec<u8>)> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).into_owned();
            let line = head.lines().next().unwrap_or("").to_string();
            return Ok((line, buf[end + 4..].to_vec()));
        }
        if buf.len() >= HEAD_MAX {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        let n = conn.read(&mut chunk)?;
        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// The only request this proxy serves.
pub fn allowed(request_line: &str) -> bool {
    let mut parts = request_line.split(' ');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some("CONNECT"), Some(target), Some(v), None)
            if target == format!("{MIRROR}:443") && v.starts_with("HTTP/1.")
    )
}

/// Serves one client connection.
pub fn handle(mut conn: TcpStream, dial: &Dial, limits: Limits) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(30)));
    let Ok((line, early)) = read_head(&mut conn) else {
        refuse(&mut conn);
        return;
    };
    if !allowed(&line) {
        refuse(&mut conn);
        return;
    }
    let Ok(mut remote) = dial(MIRROR, 443, limits.connect) else {
        let _ = conn.write_all(
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    };
    if conn
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .is_err()
        || (!early.is_empty() && remote.write_all(&early).is_err())
    {
        return;
    }
    relay(conn, remote, limits);
}

/// Copies both ways until either side closes, the byte budget is spent or
/// nothing moved in either direction for `limits.idle`.
fn relay(client: TcpStream, remote: TcpStream, limits: Limits) {
    let start = Instant::now();
    let budget = Arc::new(AtomicU64::new(limits.bytes));
    // Milliseconds since `start` of the last byte in either direction.
    let last = Arc::new(AtomicU64::new(0));
    let pump = |mut from: TcpStream, mut to: TcpStream| {
        let (budget, last) = (budget.clone(), last.clone());
        move || {
            let tick = limits.idle.min(Duration::from_secs(5));
            let _ = from.set_read_timeout(Some(tick));
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match from.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let left = budget.fetch_sub(n as u64, Ordering::SeqCst);
                        if left < n as u64 || to.write_all(&buf[..n]).is_err() {
                            break;
                        }
                        last.store(start.elapsed().as_millis() as u64, Ordering::SeqCst);
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        let quiet =
                            start.elapsed().as_millis() as u64 - last.load(Ordering::SeqCst);
                        if quiet >= limits.idle.as_millis() as u64 {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            // Ending one direction ends the tunnel.
            let _ = from.shutdown(Shutdown::Both);
            let _ = to.shutdown(Shutdown::Both);
        }
    };
    let (Ok(c2), Ok(r2)) = (client.try_clone(), remote.try_clone()) else {
        return;
    };
    let up = thread::spawn(pump(client, remote));
    pump(r2, c2)();
    let _ = up.join();
}

/// Accepts clients until the listener fails.
pub fn serve(listener: TcpListener, dial: Arc<Dial>, limits: Limits) {
    let open = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut conn) = conn else { continue };
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_TUNNELS {
            open.fetch_sub(1, Ordering::SeqCst);
            refuse(&mut conn);
            continue;
        }
        let (dial, open) = (dial.clone(), open.clone());
        let spawned = thread::Builder::new()
            .name("av-tunnel".into())
            .stack_size(128 * 1024)
            .spawn(move || {
                handle(conn, &*dial, limits);
                open.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            // Could not start a thread: the connection was dropped with it.
            continue;
        }
    }
}

static SIGNAL: AtomicI32 = AtomicI32::new(0);
extern "C" fn on_signal(sig: libc::c_int) {
    SIGNAL.store(sig, Ordering::SeqCst);
}

/// Runs FreshClam in the foreground behind the proxy; returns its status.
pub fn run() -> io::Result<ExitStatus> {
    let listener = TcpListener::bind(("127.0.0.1", PORT))?;
    thread::Builder::new()
        .name("av-proxy".into())
        .spawn(move || serve(listener, Arc::new(dial_mirror), Limits::default()))?;
    let mut command = Command::new("freshclam");
    command.args([
        "--daemon",
        "--foreground",
        "--config-file=/usr/local/share/audeniq/freshclam.conf",
    ]);
    // NO_PROXY must not bypass the explicit local transport policy.
    for (k, _) in std::env::vars_os() {
        if let Some(k) = k.to_str()
            && matches!(
                k.to_ascii_lowercase().as_str(),
                "http_proxy" | "https_proxy" | "all_proxy" | "no_proxy"
            )
        {
            command.env_remove(k);
        }
    }
    let mut child = command.spawn()?;
    // SAFETY: the handler only stores into an atomic.
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let sig = SIGNAL.swap(0, Ordering::SeqCst);
        if sig != 0 {
            // SAFETY: signalling our own child process by its pid.
            unsafe { libc::kill(child.id() as libc::pid_t, sig) };
        }
        thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn limits() -> Limits {
        Limits {
            bytes: 1024 * 1024,
            idle: Duration::from_millis(600),
            connect: Duration::from_secs(2),
        }
    }

    /// The proxy on a free port; `dial` records every upstream connection.
    fn proxy(dial: Arc<Dial>) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || serve(listener, dial, limits()));
        addr
    }

    fn response_head(c: &mut TcpStream) -> String {
        let mut out = Vec::new();
        let mut b = [0u8; 1];
        while !out.ends_with(b"\r\n\r\n") {
            if c.read(&mut b).unwrap() == 0 {
                break;
            }
            out.push(b[0]);
        }
        String::from_utf8(out).unwrap()
    }

    type Calls = Arc<Mutex<Vec<(String, u16)>>>;

    fn never_dial() -> (Arc<Dial>, Calls) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let dial: Arc<Dial> = Arc::new(move |h: &str, p: u16, _t: Duration| {
            seen.lock().unwrap().push((h.to_string(), p));
            Err(io::Error::from(io::ErrorKind::ConnectionRefused))
        });
        (dial, calls)
    }

    #[test]
    fn plaintext_never_opens_an_external_connection() {
        let (dial, calls) = never_dial();
        let addr = proxy(dial);
        for method in ["GET", "HEAD", "POST"] {
            let mut c = TcpStream::connect(addr).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            write!(
                c,
                "{method} http://{MIRROR}/daily.cvd HTTP/1.1\r\nHost: {MIRROR}\r\n\r\n"
            )
            .unwrap();
            assert!(response_head(&mut c).contains(" 403 "), "{method}");
        }
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn tunnels_cannot_target_other_hosts_or_plaintext_ports() {
        let (dial, calls) = never_dial();
        let addr = proxy(dial);
        for target in [
            "database.clamav.net:80",
            "attacker.example:443",
            "127.0.0.1:5432",
            "database.clamav.net:443@attacker.example",
            "DATABASE.CLAMAV.NET:443 HTTP/1.1 x",
        ] {
            let mut c = TcpStream::connect(addr).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            write!(c, "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").unwrap();
            assert!(response_head(&mut c).contains(" 403 "), "{target}");
        }
        // An endless head is refused too.
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = c.write_all(&vec![b'a'; HEAD_MAX + 10]);
        assert!(response_head(&mut c).contains(" 403 "));
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn the_mirror_tunnel_relays_both_ways_until_idle() {
        // Upstream: echoes what it gets, after the bytes sent with the CONNECT.
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let up_addr = upstream.local_addr().unwrap();
        thread::spawn(move || {
            let (mut s, _) = upstream.accept().unwrap();
            let mut buf = [0u8; 64];
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 || s.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        });
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let dial: Arc<Dial> = Arc::new(move |h: &str, p: u16, _t: Duration| {
            seen.lock().unwrap().push((h.to_string(), p));
            TcpStream::connect(up_addr)
        });
        let addr = proxy(dial);
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        write!(
            c,
            "CONNECT {MIRROR}:443 HTTP/1.1\r\nHost: {MIRROR}:443\r\n\r\nhello"
        )
        .unwrap();
        assert!(response_head(&mut c).starts_with("HTTP/1.1 200 "));
        let mut got = [0u8; 5];
        c.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"hello", "bytes sent with the CONNECT are not lost");
        c.write_all(b"again").unwrap();
        c.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"again");
        assert_eq!(*calls.lock().unwrap(), vec![(MIRROR.to_string(), 443)]);
        // Quiet in both directions: the tunnel closes.
        let started = Instant::now();
        let mut rest = Vec::new();
        let _ = c.read_to_end(&mut rest);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn request_line() {
        assert!(allowed("CONNECT database.clamav.net:443 HTTP/1.1"));
        assert!(allowed("CONNECT database.clamav.net:443 HTTP/1.0"));
        assert!(!allowed("CONNECT database.clamav.net:443"));
        assert!(!allowed("GET database.clamav.net:443 HTTP/1.1"));
        assert!(!allowed("CONNECT  database.clamav.net:443 HTTP/1.1"));
    }
}
