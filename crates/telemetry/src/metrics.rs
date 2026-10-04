//! Metrics in the Prometheus text format (D53). A service builds its metrics from its own
//! counters when scraped, so nothing here touches a core's decisions or reads a clock.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Counter,
    Gauge,
}

/// One metric and its labelled values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metric {
    name: &'static str,
    help: &'static str,
    kind: Kind,
    samples: Vec<(String, u64)>,
}

impl Metric {
    pub fn counter(name: &'static str, help: &'static str) -> Self {
        Self::new(name, help, Kind::Counter)
    }

    pub fn gauge(name: &'static str, help: &'static str) -> Self {
        Self::new(name, help, Kind::Gauge)
    }

    fn new(name: &'static str, help: &'static str, kind: Kind) -> Self {
        Self {
            name,
            help,
            kind,
            samples: Vec::new(),
        }
    }

    /// Adds a value with no labels.
    pub fn value(self, value: u64) -> Self {
        self.labelled(&[], value)
    }

    /// Adds a value for one combination of labels.
    pub fn labelled(mut self, labels: &[(&str, &str)], value: u64) -> Self {
        let labels = labels
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", escape(v)))
            .collect::<Vec<_>>()
            .join(",");
        self.samples.push((labels, value));
        self
    }
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// The text a scrape of `/metrics` returns.
pub fn render(metrics: &[Metric]) -> String {
    let mut out = String::new();
    for metric in metrics {
        let kind = match metric.kind {
            Kind::Counter => "counter",
            Kind::Gauge => "gauge",
        };
        out.push_str(&format!(
            "# HELP {name} {help}\n# TYPE {name} {kind}\n",
            name = metric.name,
            help = metric.help,
        ));
        for (labels, value) in &metric.samples {
            if labels.is_empty() {
                out.push_str(&format!("{} {value}\n", metric.name));
            } else {
                out.push_str(&format!("{}{{{labels}}} {value}\n", metric.name));
            }
        }
    }
    out
}

/// Serves `render()` at `GET /metrics` on its own thread, for a binary with no HTTP server of its
/// own. Returns the address it bound, so `:0` works. One scrape at a time: Prometheus is the only
/// client.
pub fn serve(
    listen: SocketAddr,
    render: impl Fn() -> String + Send + 'static,
) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(listen)?;
    let bound = listener.local_addr()?;
    std::thread::Builder::new()
        .name("metrics".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                // A client that never finishes its request is dropped, not waited on.
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let _ = answer(stream, &render);
            }
        })?;
    Ok(bound)
}

fn answer(mut stream: std::net::TcpStream, render: &impl Fn() -> String) -> std::io::Result<()> {
    // The whole header is read before answering, or closing would reset the client's connection.
    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 8192 {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let request = String::from_utf8_lossy(&buf);
    let line = request.lines().next().unwrap_or_default();
    let (status, content_type, body) = if line.starts_with("GET /metrics ") {
        ("200 OK", "text/plain; version=0.0.4", render())
    } else {
        ("404 Not Found", "text/plain", String::new())
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
