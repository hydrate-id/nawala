// Embedded TrustPositif/Komdigi national blocklist.
// Snapshot is gzip-embedded at compile time, inflated once (lazy) into a
// sorted byte buffer: one lowercase domain per line. Lookup = binary search
// over lines; a domain is blocked if it or any ancestor is listed (zone block).
use std::cmp::Ordering;
use std::io::Read;
use std::sync::OnceLock;

use flate2::read::GzDecoder;

pub const RAW_GZ: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/trustpositif.txt.gz"));

struct Blocklist {
    // sorted ascending, each line ends with '\n' (last line included)
    buf: Vec<u8>,
}

static LOADED: OnceLock<Result<Blocklist, String>> = OnceLock::new();

fn load() -> &'static Result<Blocklist, String> {
    LOADED.get_or_init(|| {
        let mut buf = Vec::with_capacity(115_000_000);
        GzDecoder::new(RAW_GZ)
            .read_to_end(&mut buf)
            .map_err(|e| format!("failed to inflate blocklist: {e}"))?;
        if !buf.is_empty() && buf.last() != Some(&b'\n') {
            buf.push(b'\n');
        }
        Ok(Blocklist { buf })
    })
}

// Binary search for needle in the sorted line buffer. Lines are short
// (<=253B + '\n'), so scanning around the midpoint stays within a few hundred bytes.
fn contains(buf: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || buf.is_empty() {
        return false;
    }
    let n = buf.len();
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;

        let mut start = mid;
        while start > lo && buf[start - 1] != b'\n' {
            start -= 1;
        }

        let mut end = mid;
        while end < n && buf[end] != b'\n' {
            end += 1;
        }

        let line = &buf[start..end];
        match line.cmp(needle) {
            Ordering::Equal => return true,
            Ordering::Less => {
                lo = if end < n { end + 1 } else { end };
            }
            Ordering::Greater => {
                hi = start;
            }
        }
    }
    false
}

fn blocked_in(buf: &[u8], domain: &[u8]) -> bool {
    let mut start = 0;
    loop {
        if contains(buf, &domain[start..]) {
            return true;
        }
        match domain[start..].iter().position(|&b| b == b'.') {
            Some(off) => start += off + 1,
            None => return false,
        }
    }
}

// domain must be lowercase ASCII without trailing dot (see valid_domain).
pub fn is_blocked(domain: &str) -> Result<bool, String> {
    let bl = load().as_ref().map_err(|e| e.clone())?;
    Ok(blocked_in(&bl.buf, domain.as_bytes()))
}

#[cfg(test)]
pub fn total_domains() -> Result<usize, String> {
    let bl = load().as_ref().map_err(|e| e.clone())?;
    Ok(bl.buf.iter().filter(|&&b| b == b'\n').count())
}

#[cfg(test)]
mod tests {
    use super::*;

    // build sorted buffer from lines (caller sorts)
    fn buf_of(lines: &[&str]) -> Vec<u8> {
        let mut v: Vec<&str> = lines.to_vec();
        v.sort_unstable();
        let mut b = Vec::new();
        for l in v {
            b.extend_from_slice(l.as_bytes());
            b.push(b'\n');
        }
        b
    }

    #[test]
    fn contains_exact_and_edges() {
        let b = buf_of(&["a.com", "b.co.id", "x.y.example.org"]);
        assert!(contains(&b, b"a.com"));
        assert!(contains(&b, b"b.co.id"));
        assert!(contains(&b, b"x.y.example.org"));
        assert!(!contains(&b, b"a.co"));
        assert!(!contains(&b, b"b.com"));
        assert!(!contains(&b, b"com"));
        assert!(!contains(&b, b"z.com"));
        assert!(!contains(&b, b""));
        assert!(!contains(&[], b"a.com"));
    }

    #[test]
    fn contains_many_edges() {
        // exercise first/last entries so binary search hits buffer edges
        let mut lines: Vec<String> = vec!["a0.com".into(), "z9.net".into()];
        for i in 0..5000 {
            lines.push(format!("d{i:04}.com"));
        }
        lines.sort_unstable();
        let mut b = Vec::new();
        for l in &lines {
            b.extend_from_slice(l.as_bytes());
            b.push(b'\n');
        }
        assert!(contains(&b, b"a0.com"));
        assert!(contains(&b, b"z9.net"));
        assert!(contains(&b, b"d2500.com"));
        assert!(!contains(&b, b"a0.co"));
        assert!(!contains(&b, b"z9.ne"));
        assert!(!contains(&b, b"zzz.net"));
        assert!(!contains(&b, b"aaaa.com"));
    }

    #[test]
    fn parent_walk_zone_block() {
        let b = buf_of(&["example.com", "blog.example.org"]);
        assert!(blocked_in(&b, b"example.com"));
        assert!(blocked_in(&b, b"www.example.com"));
        assert!(blocked_in(&b, b"deep.www.example.com"));
        assert!(blocked_in(&b, b"blog.example.org"));
        assert!(!blocked_in(&b, b"example.org"));
        assert!(!blocked_in(&b, b"other.com"));
        assert!(!blocked_in(&b, b"com"));
    }

    // integration with the real list (~9.6M): entries from the official head
    // (definitely blocked) plus well-known clean domains
    #[test]
    fn real_list_detected() {
        let blocked = [
            "partaikomunisindonesia.wordpress.com",
            "mantanmuslim.com",
            "gudangblackmarket.com",
        ];
        let clean = ["google.com", "microsoft.com", "gstatic.com"];
        for d in blocked {
            assert_eq!(is_blocked(d).unwrap(), true, "{d} should be blocked");
        }
        for d in clean {
            assert_eq!(is_blocked(d).unwrap(), false, "{d} should be allowed");
        }
        assert!(total_domains().unwrap() > 9_000_000);
    }
}
