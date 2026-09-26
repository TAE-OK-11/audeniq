#[test]
fn dbg_check_audio_full() {
    let p = std::path::Path::new("/home/hatch/audeniq-tmp/audeniq-album10-f7c95f49-e674-4d4a-adc4-a7133a6f202c/track00.flac");
    assert!(p.exists());
    let (outcomes, fp_samples) = audeniq_core::qc::check_audio_full(p, None, Some("audio/flac"));
    for o in &outcomes {
        println!("{} {:?} {}", o.check_code, o.status, o.detail.chars().take(80).collect::<String>());
    }
    println!("fp_samples: {}", fp_samples.len());
    let m = audeniq_core::qc::probe_audio_metrics(p).unwrap();
    println!("duration: {}", m.duration_secs);
    let fp = audeniq_core::fingerprint::fingerprint_from_samples(&fp_samples).unwrap();
    println!("fp frames: {} duration: {:.1}", fp.frames.len(), fp.duration_secs);
}
