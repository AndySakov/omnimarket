//! The Prometheus text format and the scrape endpoint.

use std::io::{Read, Write};
use std::net::TcpStream;

use telemetry::metrics::{Metric, render, serve};

fn get(address: std::net::SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn metrics_render_as_prometheus_text() {
    let text = render(&[
        Metric::gauge("omnimarket_head_block", "The head block.").value(12),
        Metric::counter("omnimarket_records_total", "Records.")
            .labelled(&[("topic", "trades.base")], 3)
            .labelled(&[("topic", "a\"b\\c\nd")], 4),
    ]);
    assert_eq!(
        text,
        concat!(
            "# HELP omnimarket_head_block The head block.\n",
            "# TYPE omnimarket_head_block gauge\n",
            "omnimarket_head_block 12\n",
            "# HELP omnimarket_records_total Records.\n",
            "# TYPE omnimarket_records_total counter\n",
            "omnimarket_records_total{topic=\"trades.base\"} 3\n",
            "omnimarket_records_total{topic=\"a\\\"b\\\\c\\nd\"} 4\n",
        )
    );
}

#[test]
fn the_endpoint_serves_metrics_and_nothing_else() {
    let address = serve("127.0.0.1:0".parse().unwrap(), || "up 1\n".to_string()).unwrap();
    let ok = get(address, "/metrics");
    assert!(ok.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(ok.contains("Content-Type: text/plain; version=0.0.4\r\n"));
    assert!(ok.ends_with("\r\n\r\nup 1\n"));
    assert!(get(address, "/other").starts_with("HTTP/1.1 404 Not Found\r\n"));
}
