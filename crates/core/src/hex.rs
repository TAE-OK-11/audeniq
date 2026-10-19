//! Lowercase hex encoding — replaces the `hex` crate, of which only
//! `encode` and `decode` were used.

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Lowercase hex of `data`.
pub fn encode(data: impl AsRef<[u8]>) -> String {
    let data = data.as_ref();
    let mut out = String::with_capacity(data.len() * 2);
    for &b in data {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    out
}

/// Why a hex string could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FromHexError {
    OddLength,
    InvalidChar { index: usize },
}

impl std::fmt::Display for FromHexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OddLength => f.write_str("odd number of hex digits"),
            Self::InvalidChar { index } => write!(f, "invalid hex character at {index}"),
        }
    }
}

impl std::error::Error for FromHexError {}

fn nibble(c: u8, index: usize) -> Result<u8, FromHexError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(FromHexError::InvalidChar { index }),
    }
}

/// Bytes of a hex string (either case).
pub fn decode(text: impl AsRef<[u8]>) -> Result<Vec<u8>, FromHexError> {
    let text = text.as_ref();
    if text.len() % 2 != 0 {
        return Err(FromHexError::OddLength);
    }
    let (pairs, _) = text.as_chunks::<2>();
    pairs
        .iter()
        .enumerate()
        .map(|(i, &[hi, lo])| Ok(nibble(hi, 2 * i)? << 4 | nibble(lo, 2 * i + 1)?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_byte() {
        let all: Vec<u8> = (0..=255).collect();
        let text = encode(&all);
        assert_eq!(text.len(), 512);
        assert!(text.starts_with("000102") && text.ends_with("fdfeff"));
        assert_eq!(decode(&text).unwrap(), all);
        assert_eq!(decode(text.to_uppercase()).unwrap(), all);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(decode("abc"), Err(FromHexError::OddLength));
        assert_eq!(decode("0g"), Err(FromHexError::InvalidChar { index: 1 }));
        assert_eq!(decode(" 00"), Err(FromHexError::OddLength));
        assert_eq!(decode("+0"), Err(FromHexError::InvalidChar { index: 0 }));
        assert_eq!(decode(""), Ok(vec![]));
        assert_eq!(encode([]), "");
    }
}
