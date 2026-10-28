fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).unwrap();
    let w: u32 = a[2].parse().unwrap();
    let h: u32 = a[3].parse().unwrap();
    let quality: f32 = a[5].parse().unwrap();
    let start = std::time::Instant::now();
    let out = match a[4].as_str() {
        "c" => webp::Encoder::from_rgba(&bytes, w, h)
            .encode(quality)
            .to_vec(),
        "avif" => {
            let pixels: Vec<ravif::RGBA8> = bytes
                .chunks_exact(4)
                .map(|p| ravif::RGBA8::new(p[0], p[1], p[2], p[3]))
                .collect();
            ravif::Encoder::new()
                .with_quality(quality)
                .with_alpha_quality(100.)
                .with_speed(a[6].parse().unwrap())
                .with_num_threads(Some(1))
                .encode_rgba(ravif::Img::new(&pixels, w as usize, h as usize))
                .unwrap()
                .avif_file
        }
        _ => panic!(),
    };
    eprintln!("{}", start.elapsed().as_millis());
    std::fs::write(&a[7], out).unwrap();
}
