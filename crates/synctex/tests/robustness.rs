#![allow(clippy::pedantic)]
//! Hostile or truncated input must never panic: a SyncTeX file comes out of a
//! TeX run that may have been killed half way.

use synctex::SyncTexData;
use synctex::parser::parse_content;

const VALID: &str = "SyncTeX Version:1\nInput:1:/tmp/main.tex\nOutput:pdf\nMagnification:1000\nUnit:1\nX Offset:0\nY Offset:0\nContent:\n!100\n{1\n[1,3:4736287,4736287:26214400,40000000,0\n(1,3:4736287,5000000:26214400,500000,100\nh1,3:4736287,5000000:3000000,500000,100\n)\n]\n}1\nPostamble:\nCount:6\n";

/// Small deterministic generator, so a failure reproduces.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn mutated_content_never_panics() {
    let mut rng = Lcg(7);
    for _ in 0..2000 {
        let mut bytes = VALID.as_bytes().to_vec();
        for _ in 0..(1 + rng.next() % 8) {
            let at = (rng.next() as usize) % bytes.len();
            match rng.next() % 3 {
                0 => bytes[at] = (rng.next() % 128) as u8,
                1 => {
                    bytes.remove(at);
                }
                _ => bytes.truncate(at),
            }
            if bytes.is_empty() {
                break;
            }
        }
        let text = String::from_utf8_lossy(&bytes);
        let data = parse_content(&text);
        let _ = data.backward_search(1, 100.0, 100.0);
        let _ = data.backward_search(0, f64::NAN, f64::INFINITY);
        let _ = data.forward_search(1, 3);
        let _ = data.forward_search(u32::MAX, u32::MAX);
    }
}

#[test]
fn garbage_that_is_not_gzip_is_an_error_not_a_panic() {
    assert!(SyncTexData::parse_gz_bytes(b"definitely not gzip").is_err());
    assert!(SyncTexData::parse_gz_bytes(&[]).is_err());
    assert!(SyncTexData::parse_gz_bytes(&[0x1f, 0x8b, 0x08, 0, 0, 0]).is_err());
}
