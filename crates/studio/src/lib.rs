//! Browser application logic is Rust. JavaScript is generated WASM loader glue only.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn audio_mime(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    if n.ends_with(".wav") {
        Some("audio/wav")
    } else if n.ends_with(".flac") {
        Some("audio/flac")
    } else {
        None
    }
}
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(test)]
mod tests {
    #[test]
    fn untrusted_metadata_is_escaped() {
        assert_eq!(
            super::escape("<img onerror='x'>&\""),
            "&lt;img onerror=&#39;x&#39;&gt;&amp;&quot;"
        );
        assert_eq!(super::audio_mime("Track.WAV"), Some("audio/wav"));
        assert_eq!(super::audio_mime("Track.wav.exe"), None);
    }
}
