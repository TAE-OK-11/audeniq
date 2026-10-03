#![allow(dead_code)]
mod error {
    #[derive(Debug)]
    pub enum Error {
        Internal,
        PolicyGate(&'static str),
    }
    pub type Result<T> = std::result::Result<T, Error>;
}
#[path = "parser_sandbox.rs"]
mod parser_sandbox;
#[path = "baseline-fingerprint.rs"]
mod baseline;
#[path = "updated-fingerprint.rs"]
mod updated;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn measure(mut f: impl FnMut(), n: usize) -> Duration {
    let start = Instant::now();
    for _ in 0..n {
        f();
    }
    start.elapsed()
}

fn main() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed as u32
    };
    let a: Vec<u32> = (0..967).map(|_| next()).collect();
    let unrelated: Vec<u32> = (0..967).map(|_| next()).collect();
    let noisy: Vec<u32> = a
        .iter()
        .enumerate()
        .map(|(i, f)| if i % 8 == 0 { f ^ 0x5555_5555 } else { *f })
        .collect();
    let shifted: Vec<u32> = unrelated[..80]
        .iter()
        .chain(a[..887].iter())
        .copied()
        .collect();
    for (name, other) in [
        ("identical", &a),
        ("lightly edited", &noisy),
        ("shifted", &shifted),
        ("unrelated", &unrelated),
    ] {
        assert_eq!(
            baseline::bit_error_rate_within(&a, other, 0.25),
            updated::bit_error_rate_within(&a, other, 0.25)
        );
        let n = 300;
        let old = measure(
            || {
                black_box(baseline::bit_error_rate_within(
                    black_box(&a),
                    black_box(other),
                    0.25,
                ));
            },
            n,
        );
        let new = measure(
            || {
                black_box(updated::bit_error_rate_within(
                    black_box(&a),
                    black_box(other),
                    0.25,
                ));
            },
            n,
        );
        println!(
            "{name}: n={n} baseline_ms={:.3} updated_ms={:.3} speedup={:.2}x",
            old.as_secs_f64() * 1000.0,
            new.as_secs_f64() * 1000.0,
            old.as_secs_f64() / new.as_secs_f64()
        );
    }
    let pcm: Vec<f32> = (0..30 * 11025)
        .map(|_| (next() as i32 as f32) / (i32::MAX as f32) * 0.5)
        .collect();
    let segments: Vec<&[f32]> = vec![&pcm, &pcm, &pcm];
    assert_eq!(
        baseline::fingerprint_from_segments(&segments)
            .unwrap()
            .frames,
        updated::fingerprint_from_segments(&segments)
            .unwrap()
            .frames
    );
    let n = 15;
    let old = measure(
        || {
            black_box(baseline::fingerprint_from_segments(black_box(&segments)).unwrap());
        },
        n,
    );
    let new = measure(
        || {
            black_box(updated::fingerprint_from_segments(black_box(&segments)).unwrap());
        },
        n,
    );
    println!(
        "3x30s FFT: n={n} baseline_ms={:.3} updated_ms={:.3} speedup={:.2}x; fingerprints bit-identical",
        old.as_secs_f64() * 1000.0,
        new.as_secs_f64() * 1000.0,
        old.as_secs_f64() / new.as_secs_f64()
    );
}
