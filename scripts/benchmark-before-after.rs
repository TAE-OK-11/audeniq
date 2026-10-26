// Before (917861d) and now, same machine and inputs: profanity filter,
// protected artist names, fingerprint report parsing. Run with
// scripts/benchmark-before-after.sh, which extracts the old sources.
#![allow(dead_code)]
#[path = "old_profanity/mod.rs"]
mod old_profanity;
#[path = "new_profanity/mod.rs"]
mod new_profanity;
#[path = "old_names.rs"]
mod old_names;
#[path = "new_names.rs"]
mod new_names;
mod seed;
use serde::Deserialize;
use std::{hint::black_box, time::Instant};

fn median_us(iters: usize, mut f: impl FnMut()) -> f64 {
    for _ in 0..iters.min(20) {
        f();
    }
    let mut runs: Vec<f64> = (0..7)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..iters {
                f();
            }
            start.elapsed().as_secs_f64() * 1e6 / iters as f64
        })
        .collect();
    runs.sort_by(f64::total_cmp);
    runs[3]
}

fn report(name: &str, old: f64, new: f64, unit: &str) {
    println!(
        "{name} ({}): before {old:.1} {unit}, now {new:.1} {unit} ({:.1}x)",
        std::env::consts::ARCH,
        old / new
    );
}

macro_rules! entries {
    ($m:ident) => {{
        seed::PROT
            .iter()
            .map(|(n, mode, action)| {
                let specs: Vec<$m::NameSpec> = seed::ALIAS
                    .iter()
                    .filter(|a| a.0 == *n)
                    .map(|a| $m::NameSpec::new(a.1, $m::Mode::parse(a.2), $m::Action::parse(a.3)))
                    .collect();
                $m::Entry::with_policy(
                    $m::NameSpec::new(n, $m::Mode::parse(mode), $m::Action::parse(action)),
                    &specs,
                )
            })
            .collect::<Vec<_>>()
    }};
}

#[derive(Deserialize)]
struct OldWindow {
    start_secs: f64,
    samples: Vec<i16>,
}
#[derive(Deserialize)]
struct OldReport {
    fingerprint_windows: Vec<OldWindow>,
}
// Now: crate::qc_engine reads s16 straight into f32 (same visitor).
fn s16_as_f32<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<f32>, D::Error> {
    struct Samples;
    impl<'de> serde::de::Visitor<'de> for Samples {
        type Value = Vec<f32>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("s16 samples")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<f32>, A::Error> {
            let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1 << 20));
            while let Some(s) = seq.next_element::<i16>()? {
                out.push(f32::from(s) / 32768.0);
            }
            Ok(out)
        }
    }
    d.deserialize_seq(Samples)
}
#[derive(Deserialize)]
struct NewWindow {
    start_secs: f64,
    #[serde(deserialize_with = "s16_as_f32")]
    samples: Vec<f32>,
}
#[derive(Deserialize)]
struct NewReport {
    fingerprint_windows: Vec<NewWindow>,
}

fn main() {
    // Profanity: identical verdicts, then time.
    let cheer = "오빠 이번 앨범 진짜 최고예요!! 항상 응원합니다 ❤️";
    let lyrics = "밤하늘의 별처럼 너를 기다려 오늘도 나는 이 길을 걸어 the night is young and so are we ".repeat(40);
    let words: Vec<&str> = old_profanity::words::KORCEN.iter().chain(old_profanity::words::KORCEN_ALLOWED).copied().collect();
    for w in &words {
        for t in [w.to_string(), format!("가{w}나"), format!("시 {w}"), format!("{w} {w}")] {
            assert_eq!(old_profanity::contains(&t, &[]), new_profanity::contains(&t, &[]), "{t:?}");
        }
    }
    for (name, text, iters) in [("profanity cheer 60 chars", cheer.to_string(), 2000), ("profanity lyrics 3.6k chars", lyrics, 100)] {
        let old = median_us(iters, || {
            black_box(old_profanity::contains(black_box(&text), &[]));
        });
        let new = median_us(iters, || {
            black_box(new_profanity::contains(black_box(&text), &[]));
        });
        report(name, old, new, "us");
    }

    // Protected names: what one enforce() does (build entries + find).
    let texts = ["Midnight over the ocean", "Original Independent Artist", "봄날의 작은 노래", "새벽 감성 (feat. 김민수)", "Remastered 2024", "독립 레이블 하늘소리"];
    let (eo, en) = (entries!(old_names), entries!(new_names));
    for t in texts.iter().chain(["Taylor Swift", "테일러 스위프트의", "T4ylor $wift", "BTS"].iter()) {
        assert_eq!(
            old_names::find(&eo, &[t]).map(|e| e.name.clone()),
            new_names::find(&en, &[t]).map(|e| e.name.clone()),
            "{t:?}"
        );
    }
    let old = median_us(300, || {
        let e = entries!(old_names);
        black_box(old_names::find(&e, black_box(&texts)).is_some());
    });
    let new = median_us(300, || {
        let e = entries!(new_names);
        black_box(new_names::find(&e, black_box(&texts)).is_some());
    });
    report("protected names enforce", old, new, "us");

    // Fingerprint report: 3 x 30 s of 11025 Hz s16 as JSON integers.
    let mut doc = String::from("{\"fingerprint_windows\":[");
    let mut seed = 99u32;
    for w in 0..3 {
        doc.push_str(&format!("{}{{\"start_secs\":{}.0,\"samples\":[", if w > 0 { "," } else { "" }, w * 60));
        for i in 0..330_750 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let t = i as f64 / 11025.0;
            let v = ((2.0 * std::f64::consts::PI * 440.0 * t).sin() * 9000.0 + (seed as f64 / u32::MAX as f64 - 0.5) * 6000.0) as i16;
            doc.push_str(&format!("{}{v}", if i > 0 { "," } else { "" }));
        }
        doc.push_str("]}");
    }
    doc.push_str("]}");
    let to_f32 = |r: OldReport| -> Vec<Vec<f32>> {
        r.fingerprint_windows.iter().map(|w| w.samples.iter().map(|s| f32::from(*s) / 32768.0).collect()).collect()
    };
    let a = to_f32(serde_json::from_str(&doc).unwrap());
    let b: Vec<Vec<f32>> = serde_json::from_str::<NewReport>(&doc).unwrap().fingerprint_windows.into_iter().map(|w| w.samples).collect();
    assert!(a == b);
    let old = median_us(10, || {
        black_box(to_f32(serde_json::from_str(black_box(&doc)).unwrap()));
    });
    let new = median_us(10, || {
        black_box(serde_json::from_str::<NewReport>(black_box(&doc)).unwrap().fingerprint_windows.len());
    });
    report("fingerprint report parse (5 MB)", old / 1000.0, new / 1000.0, "ms");
    println!(
        "fingerprint report parse: before keeps the s16 copy too (+{} KB per analysis)",
        3 * 330_750 * 2 / 1024
    );
}
