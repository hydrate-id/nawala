// Rebuild data/trustpositif.txt.gz from the official Komdigi list.
// Run before deploy / to update the list: cargo run --release --example refresh
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use flate2::write::GzEncoder;
use flate2::Compression;

const SOURCE: &str = "https://trustpositif.komdigi.go.id/assets/db/domains_isp";

// raw line -> Some(domain) if it belongs in the list (mirrors valid_domain)
fn clean(raw: &str) -> Option<String> {
    let d = raw.trim().trim_matches('\r').to_ascii_lowercase();
    if d.is_empty() || d.len() > 253 || d.contains("://") || d.contains('/') {
        return None;
    }
    let labels: Vec<&str> = d.split('.').collect();
    if labels.len() < 2 {
        return None;
    }
    for (i, label) in labels.iter().enumerate() {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return None;
        }
        if i == labels.len() - 1
            && (label.len() < 2 || !label.bytes().all(|b| b.is_ascii_alphabetic()))
        {
            return None;
        }
    }
    Some(d)
}

fn main() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/trustpositif.txt.gz");

    println!("downloading {SOURCE}");
    let start = Instant::now();
    let resp = ureq::get(SOURCE)
        .set("User-Agent", "Mozilla/5.0")
        .timeout(Duration::from_secs(300))
        .call()
        .unwrap_or_else(|e| {
            eprintln!("download failed: {e}");
            std::process::exit(1);
        });
    let mut raw = Vec::with_capacity(120_000_000);
    resp.into_reader()
        .read_to_end(&mut raw)
        .unwrap_or_else(|e| {
            eprintln!("failed to read response: {e}");
            std::process::exit(1);
        });
    let body = String::from_utf8_lossy(&raw).into_owned();
    drop(raw);
    let raw_count = body.lines().count();

    let mut domains: Vec<String> = Vec::with_capacity(raw_count);
    for line in body.lines() {
        if let Some(d) = clean(line) {
            domains.push(d);
        }
    }
    drop(body);

    domains.sort();
    domains.dedup();
    println!(
        "raw {raw_count} lines -> {} valid (unique) in {:.1}s",
        domains.len(),
        start.elapsed().as_secs_f32()
    );

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("create data dir");
    }
    let file = File::create(&out).expect("open output file");
    let mut enc = GzEncoder::new(BufWriter::new(file), Compression::best());
    for d in &domains {
        enc.write_all(d.as_bytes()).expect("write");
        enc.write_all(b"\n").expect("write");
    }
    let writer = enc.finish().expect("flush gzip");
    let size = writer.into_inner().expect("get file").metadata().unwrap().len();
    println!("wrote {} ({} MB) in {:.1}s", out.display(), size / 1_048_576, start.elapsed().as_secs_f32());
}
