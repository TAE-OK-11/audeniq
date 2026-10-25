// MD5 throughput: crate::md5 against RustCrypto md-5 (the crate it replaced),
// both at rustc -O. Run with scripts/benchmark-md5.sh.
#[path = "../crates/core/src/md5.rs"]
mod audeniq_md5;
use md5::Digest;
use std::{hint::black_box, time::Instant};

fn main() {
    for (size, iters) in [
        (64usize, 400_000usize),
        (1024, 80_000),
        (65536, 1500),
        (16 << 20, 6),
    ] {
        let mut s = 7u32;
        let data: Vec<u8> = (0..size)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                s as u8
            })
            .collect();
        let want: [u8; 16] = md5::Md5::digest(&data).into();
        assert_eq!(audeniq_md5::Md5::digest(&data), want);
        let (mut old, mut new) = (Vec::new(), Vec::new());
        // Alternate the order; 7 rounds, median.
        for round in 0..7 {
            for which in [round % 2, 1 - round % 2] {
                let start = Instant::now();
                for _ in 0..iters {
                    if which == 0 {
                        black_box(md5::Md5::digest(black_box(&data)));
                    } else {
                        black_box(audeniq_md5::Md5::digest(black_box(&data)));
                    }
                }
                let mbps = (size * iters) as f64 / start.elapsed().as_secs_f64() / 1e6;
                if which == 0 {
                    old.push(mbps)
                } else {
                    new.push(mbps)
                }
            }
        }
        old.sort_by(f64::total_cmp);
        new.sort_by(f64::total_cmp);
        println!(
            "md5 {size} B: md-5 {:.0} MB/s, audeniq {:.0} MB/s ({:+.1}%)",
            old[3],
            new[3],
            (new[3] / old[3] - 1.0) * 100.0
        );
    }
}
