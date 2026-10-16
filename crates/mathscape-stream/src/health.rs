use crate::writer::{EpochReport, MechanismEvent};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Default)]
pub struct Gauges {
    seed: u64,
    ready: AtomicBool,
    resumed: AtomicBool,
    epoch: AtomicU64,
    library: AtomicU64,
    certified: AtomicU64,
    rejected: AtomicU64,
    candidates: AtomicU64,
    mutations: AtomicU64,
    saturations: AtomicU64,
    frontier_cursor: AtomicU64,
    frontier_size: AtomicU64,
    emergent: AtomicBool,
    enumeration_size: AtomicU64,
    enumerated: AtomicU64,
    held: AtomicU64,
    named: AtomicU64,
    started_at: AtomicU64,
    last_epoch_at: AtomicU64,
    last_snapshot_at: AtomicU64,
    last_certified_at: AtomicU64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Gauges {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let g = Self {
            seed,
            ..Self::default()
        };
        g.started_at.store(now(), Ordering::Relaxed);
        g
    }

    pub fn opened(&self, epoch: u64, library: usize, resumed: bool) {
        self.epoch.store(epoch, Ordering::Relaxed);
        self.library.store(library as u64, Ordering::Relaxed);
        self.resumed.store(resumed, Ordering::Relaxed);
        self.ready.store(true, Ordering::Release);
    }

    pub fn observe(&self, r: &EpochReport) {
        let t = now();
        self.epoch.store(r.epoch, Ordering::Relaxed);
        self.library.store(r.library as u64, Ordering::Relaxed);
        self.certified.fetch_add(r.certified as u64, Ordering::Relaxed);
        self.rejected.fetch_add(r.rejected as u64, Ordering::Relaxed);
        self.candidates.fetch_add(r.candidates as u64, Ordering::Relaxed);
        if let Some((cursor, size)) = r.frontier {
            self.frontier_cursor.store(cursor as u64, Ordering::Relaxed);
            self.frontier_size.store(size as u64, Ordering::Relaxed);
        }
        if let Some(e) = r.emergence {
            self.emergent.store(true, Ordering::Relaxed);
            self.enumeration_size.store(e.size as u64, Ordering::Relaxed);
            self.enumerated.fetch_add(e.enumerated, Ordering::Relaxed);
            self.held.store(e.held as u64, Ordering::Relaxed);
            self.named.fetch_add(e.named as u64, Ordering::Relaxed);
        }
        match r.mechanism {
            Some(MechanismEvent::Mutated) => {
                self.mutations.fetch_add(1, Ordering::Relaxed);
            }
            Some(MechanismEvent::Saturated) => {
                self.saturations.fetch_add(1, Ordering::Relaxed);
            }
            None => {}
        }
        if r.snapshot.is_some() {
            self.last_snapshot_at.store(t, Ordering::Relaxed);
        }
        if r.certified > 0 {
            self.last_certified_at.store(t, Ordering::Relaxed);
        }
        self.last_epoch_at.store(t, Ordering::Relaxed);
    }

    #[must_use]
    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn render(&self) -> String {
        let seed = self.seed;
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let rows: [(&str, &str, &str, u64); 20] = [
            ("mathscape_stream_up", "gauge", "1 once the writer has opened its HEAD", u64::from(self.ready())),
            ("mathscape_stream_resumed", "gauge", "1 when this process resumed from an existing HEAD", u64::from(self.resumed.load(Ordering::Relaxed))),
            ("mathscape_stream_epoch", "gauge", "last epoch the writer completed", get(&self.epoch)),
            ("mathscape_stream_library_rules", "gauge", "rules in the model's library", get(&self.library)),
            ("mathscape_stream_certified_total", "counter", "invariants certified by this process", get(&self.certified)),
            ("mathscape_stream_rejected_total", "counter", "candidates rejected at certification by this process", get(&self.rejected)),
            ("mathscape_stream_candidates_total", "counter", "candidates proposed to certification by this process", get(&self.candidates)),
            ("mathscape_stream_mechanism_mutations_total", "counter", "plateau responses that kept a mutated mechanism", get(&self.mutations)),
            ("mathscape_stream_mechanism_saturations_total", "counter", "plateau responses where no mutation helped", get(&self.saturations)),
            ("mathscape_stream_frontier_cursor", "gauge", "position in the equation frontier", get(&self.frontier_cursor)),
            ("mathscape_stream_frontier_size", "gauge", "equations the frontier currently holds", get(&self.frontier_size)),
            ("mathscape_stream_emergent", "gauge", "1 when the model grows from the kernel primitives (the emergent substrate)", u64::from(self.emergent.load(Ordering::Relaxed))),
            ("mathscape_stream_emergence_term_size", "gauge", "size of the lambda terms the emergent motor is enumerating", get(&self.enumeration_size)),
            ("mathscape_stream_emergence_enumerated_total", "counter", "primitive terms the emergent motor has observed in this process", get(&self.enumerated)),
            ("mathscape_stream_emergence_symbols", "gauge", "constants and operators the model holds", get(&self.held)),
            ("mathscape_stream_emergence_named_total", "counter", "constants and operators named by this process", get(&self.named)),
            ("mathscape_stream_started_timestamp_seconds", "gauge", "when this process started", get(&self.started_at)),
            ("mathscape_stream_last_epoch_timestamp_seconds", "gauge", "when the last epoch completed (0 before the first)", get(&self.last_epoch_at)),
            ("mathscape_stream_last_snapshot_timestamp_seconds", "gauge", "when HEAD last moved (0 before the first snapshot)", get(&self.last_snapshot_at)),
            ("mathscape_stream_last_certified_timestamp_seconds", "gauge", "when an invariant was last certified (0 before the first)", get(&self.last_certified_at)),
        ];
        let mut out = String::new();
        for (name, kind, help, value) in rows {
            out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n{name}{{seed=\"{seed}\"}} {value}\n"));
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Healthz,
    Readyz,
    Metrics,
    NotFound,
}

impl Route {
    fn of(request_line: &str) -> Self {
        let mut parts = request_line.split_whitespace();
        let (Some("GET" | "HEAD"), Some(target)) = (parts.next(), parts.next()) else {
            return Self::NotFound;
        };
        match target.split('?').next() {
            Some("/healthz") => Self::Healthz,
            Some("/readyz") => Self::Readyz,
            Some("/metrics") => Self::Metrics,
            _ => Self::NotFound,
        }
    }
}

fn respond(stream: &mut TcpStream, gauges: &Gauges) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut line = String::new();
    BufReader::new(&*stream).read_line(&mut line)?;
    let (status, content_type, body) = match Route::of(&line) {
        Route::Healthz => ("200 OK", "text/plain", "ok\n".to_owned()),
        Route::Readyz if gauges.ready() => ("200 OK", "text/plain", "ready\n".to_owned()),
        Route::Readyz => ("503 Service Unavailable", "text/plain", "opening HEAD\n".to_owned()),
        Route::Metrics => ("200 OK", "text/plain; version=0.0.4", gauges.render()),
        Route::NotFound => ("404 Not Found", "text/plain", "not found\n".to_owned()),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

pub struct Health {
    addr: std::net::SocketAddr,
    _thread: JoinHandle<()>,
}

impl Health {
    pub fn serve(addr: impl ToSocketAddrs, gauges: Arc<Gauges>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let addr = listener.local_addr()?;
        let thread = std::thread::Builder::new()
            .name("stream-health".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    if let Ok(mut stream) = conn {
                        if let Err(e) = respond(&mut stream, &gauges) {
                            tracing::debug!("health: {e}");
                        }
                    }
                }
            })?;
        Ok(Self {
            addr,
            _thread: thread,
        })
    }

    #[must_use]
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn get(addr: std::net::SocketAddr, path: &str) -> (String, String) {
        let mut s = TcpStream::connect(addr).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut raw = String::new();
        s.read_to_string(&mut raw).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        (head.lines().next().unwrap().to_owned(), body.to_owned())
    }

    fn report(epoch: u64, certified: usize, snapshot: bool) -> EpochReport {
        EpochReport {
            epoch,
            candidates: 10,
            certified,
            rejected: 1,
            library: 40,
            snapshot: snapshot.then(|| mathscape_core::hash::TermRef::from_bytes(b"head")),
            frontier: Some((7, 99)),
            mechanism: Some(MechanismEvent::Mutated),
            emergence: None,
        }
    }

    #[test]
    fn readiness_waits_for_the_head_and_liveness_does_not() {
        let g = Arc::new(Gauges::new(7));
        let h = Health::serve("127.0.0.1:0", Arc::clone(&g)).unwrap();
        assert_eq!(get(h.local_addr(), "/healthz").0, "HTTP/1.1 200 OK");
        assert_eq!(get(h.local_addr(), "/readyz").0, "HTTP/1.1 503 Service Unavailable");
        g.opened(12, 40, true);
        assert_eq!(get(h.local_addr(), "/readyz").0, "HTTP/1.1 200 OK");
        assert_eq!(get(h.local_addr(), "/nope").0, "HTTP/1.1 404 Not Found");
    }

    #[test]
    fn metrics_accumulate_what_each_epoch_reports() {
        let g = Arc::new(Gauges::new(2026));
        g.opened(0, 0, false);
        g.observe(&report(1, 3, false));
        g.observe(&report(2, 2, true));
        let h = Health::serve("127.0.0.1:0", Arc::clone(&g)).unwrap();
        let (status, body) = get(h.local_addr(), "/metrics");
        assert_eq!(status, "HTTP/1.1 200 OK");
        for want in [
            "mathscape_stream_up{seed=\"2026\"} 1",
            "mathscape_stream_epoch{seed=\"2026\"} 2",
            "mathscape_stream_certified_total{seed=\"2026\"} 5",
            "mathscape_stream_rejected_total{seed=\"2026\"} 2",
            "mathscape_stream_library_rules{seed=\"2026\"} 40",
            "mathscape_stream_mechanism_mutations_total{seed=\"2026\"} 2",
            "mathscape_stream_frontier_size{seed=\"2026\"} 99",
            "# TYPE mathscape_stream_certified_total counter",
        ] {
            assert!(body.contains(want), "missing `{want}` in\n{body}");
        }
        assert!(!body.contains("mathscape_stream_last_snapshot_timestamp_seconds{seed=\"2026\"} 0\n"));
    }
}
