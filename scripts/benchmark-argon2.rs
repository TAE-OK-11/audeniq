// Argon2id with the production profile (19 MiB, t=2, p=1): crate::argon2id
// against the argon2 crate, both at rustc -O. Run with scripts/benchmark-argon2.sh.
#[allow(dead_code)]
#[path = "../crates/core/src/argon2id.rs"]
mod audeniq_argon2id;
use argon2::{Algorithm, Argon2, Params, Version};
use std::{hint::black_box, time::Instant};

fn reference(password: &[u8], salt: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19 * 1024, 2, 1, Some(32)).unwrap(),
    )
    .hash_password_into(password, salt, &mut out)
    .unwrap();
    out
}

fn main() {
    let salt = b"0123456789abcdef";
    for i in 0..3 {
        let pw = format!("Long-test-password-{i}");
        assert_eq!(
            audeniq_argon2id::hash(pw.as_bytes(), salt, 19 * 1024, 2, 1),
            Some(reference(pw.as_bytes(), salt))
        );
    }
    let (mut old, mut new) = (Vec::new(), Vec::new());
    for round in 0..21 {
        let pw = format!("pw{round}");
        for which in [round % 2, 1 - round % 2] {
            let start = Instant::now();
            if which == 0 {
                black_box(reference(black_box(pw.as_bytes()), salt));
            } else {
                black_box(audeniq_argon2id::hash(
                    black_box(pw.as_bytes()),
                    salt,
                    19 * 1024,
                    2,
                    1,
                ));
            }
            let ms = start.elapsed().as_secs_f64() * 1e3;
            if which == 0 {
                old.push(ms)
            } else {
                new.push(ms)
            }
        }
    }
    old.sort_by(f64::total_cmp);
    new.sort_by(f64::total_cmp);
    println!(
        "argon2id 19MiB t=2 p=1 ({}): argon2 crate {:.2} ms, audeniq {:.2} ms ({:+.1}%)",
        std::env::consts::ARCH,
        old[10],
        new[10],
        (new[10] / old[10] - 1.0) * 100.0
    );
}
