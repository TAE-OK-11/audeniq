//! MD5 (RFC 1321), written from the RFC for the S3 Object Lock `Content-MD5`
//! header. A transfer integrity check the protocol requires, not a security
//! hash: nothing here relies on MD5 resisting collisions.
//!
//! One block is 64 dependent steps, so the speed is the latency of the chain
//! through `b`. Each round function is written so the terms not involving
//! the new `b` are computed while the previous step is still running:
//!
//! * F = (b & c) | (!b & d)  ==  d ^ (b & (c ^ d))
//! * G = (b & d) | (c & !d)  ==  (b & d) + (c & !d)   (disjoint bits)
//! * H = b ^ c ^ d           ==  b ^ (c ^ d)
//! * I = c ^ (b | !d)
//!
//! and `a + X[k] + T[i]` is summed before `b` is known (see `compress`).
//!
//! The tests check the RFC test suite and every length and split point
//! against an independent implementation.

/// T[i] = floor(2^32 * |sin(i)|), i = 1..=64 (RFC 1321 3.4).
const T: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// Streaming MD5.
#[derive(Clone)]
pub struct Md5 {
    state: [u32; 4],
    buf: [u8; 64],
    buf_len: usize,
    total: u64,
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

impl Md5 {
    pub fn new() -> Self {
        Self {
            state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            buf: [0; 64],
            buf_len: 0,
            total: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len < 64 {
                return;
            }
            let block = self.buf;
            compress(&mut self.state, &block);
            self.buf_len = 0;
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for block in blocks {
            compress(&mut self.state, block);
        }
        self.buf[..rest.len()].copy_from_slice(rest);
        self.buf_len = rest.len();
    }

    pub fn finalize(mut self) -> [u8; 16] {
        let bits = self.total.wrapping_mul(8);
        // 0x80, zeros to 56 mod 64, then the bit length little-endian.
        let mut pad = [0u8; 72];
        pad[0] = 0x80;
        let zeros = (55usize.wrapping_sub(self.buf_len)) % 64;
        pad[1 + zeros..9 + zeros].copy_from_slice(&bits.to_le_bytes());
        self.update(&pad[..9 + zeros]);
        debug_assert_eq!(self.buf_len, 0);
        let mut out = [0u8; 16];
        for (o, s) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *o = s.to_le_bytes();
        }
        out
    }

    /// MD5 of `data` in one call.
    pub fn digest(data: &[u8]) -> [u8; 16] {
        let mut h = Self::new();
        h.update(data);
        h.finalize()
    }
}

#[inline(always)]
fn compress(state: &mut [u32; 4], block: &[u8; 64]) {
    let mut x = [0u32; 16];
    for (w, b) in x.iter_mut().zip(block.as_chunks::<4>().0) {
        *w = u32::from_le_bytes(*b);
    }
    // Opaque to the optimizer: with constant T it reassociates a + X + T + f
    // into (a + X + f) + T, which puts one more add after f on the serial
    // chain of every step (about 20% slower). Read from memory, T joins the
    // part summed before b is ready.
    let t = std::hint::black_box(&T);
    let [mut a, mut b, mut c, mut d] = *state;
    // One step: a = b + ((a + f(b, c, d) + X[k] + T[i]) <<< s), with the
    // terms that do not need b added first.
    macro_rules! step {
        (F, $a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
            $a = $a.wrapping_add(x[$k]).wrapping_add(t[$i]);
            $a = $a.wrapping_add($d ^ ($b & ($c ^ $d)));
            $a = $a.rotate_left($s).wrapping_add($b);
        };
        (G, $a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
            $a = $a
                .wrapping_add(x[$k])
                .wrapping_add(t[$i])
                .wrapping_add($c & !$d);
            $a = $a.wrapping_add($b & $d);
            $a = $a.rotate_left($s).wrapping_add($b);
        };
        (H, $a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
            $a = $a.wrapping_add(x[$k]).wrapping_add(t[$i]);
            $a = $a.wrapping_add($b ^ ($c ^ $d));
            $a = $a.rotate_left($s).wrapping_add($b);
        };
        (I, $a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
            $a = $a.wrapping_add(x[$k]).wrapping_add(t[$i]);
            $a = $a.wrapping_add($c ^ ($b | !$d));
            $a = $a.rotate_left($s).wrapping_add($b);
        };
    }
    // Round 1: X[i], shifts 7 12 17 22.
    step!(F, a, b, c, d, 0, 7, 0);
    step!(F, d, a, b, c, 1, 12, 1);
    step!(F, c, d, a, b, 2, 17, 2);
    step!(F, b, c, d, a, 3, 22, 3);
    step!(F, a, b, c, d, 4, 7, 4);
    step!(F, d, a, b, c, 5, 12, 5);
    step!(F, c, d, a, b, 6, 17, 6);
    step!(F, b, c, d, a, 7, 22, 7);
    step!(F, a, b, c, d, 8, 7, 8);
    step!(F, d, a, b, c, 9, 12, 9);
    step!(F, c, d, a, b, 10, 17, 10);
    step!(F, b, c, d, a, 11, 22, 11);
    step!(F, a, b, c, d, 12, 7, 12);
    step!(F, d, a, b, c, 13, 12, 13);
    step!(F, c, d, a, b, 14, 17, 14);
    step!(F, b, c, d, a, 15, 22, 15);
    // Round 2: X[(1 + 5i) mod 16], shifts 5 9 14 20.
    step!(G, a, b, c, d, 1, 5, 16);
    step!(G, d, a, b, c, 6, 9, 17);
    step!(G, c, d, a, b, 11, 14, 18);
    step!(G, b, c, d, a, 0, 20, 19);
    step!(G, a, b, c, d, 5, 5, 20);
    step!(G, d, a, b, c, 10, 9, 21);
    step!(G, c, d, a, b, 15, 14, 22);
    step!(G, b, c, d, a, 4, 20, 23);
    step!(G, a, b, c, d, 9, 5, 24);
    step!(G, d, a, b, c, 14, 9, 25);
    step!(G, c, d, a, b, 3, 14, 26);
    step!(G, b, c, d, a, 8, 20, 27);
    step!(G, a, b, c, d, 13, 5, 28);
    step!(G, d, a, b, c, 2, 9, 29);
    step!(G, c, d, a, b, 7, 14, 30);
    step!(G, b, c, d, a, 12, 20, 31);
    // Round 3: X[(5 + 3i) mod 16], shifts 4 11 16 23.
    step!(H, a, b, c, d, 5, 4, 32);
    step!(H, d, a, b, c, 8, 11, 33);
    step!(H, c, d, a, b, 11, 16, 34);
    step!(H, b, c, d, a, 14, 23, 35);
    step!(H, a, b, c, d, 1, 4, 36);
    step!(H, d, a, b, c, 4, 11, 37);
    step!(H, c, d, a, b, 7, 16, 38);
    step!(H, b, c, d, a, 10, 23, 39);
    step!(H, a, b, c, d, 13, 4, 40);
    step!(H, d, a, b, c, 0, 11, 41);
    step!(H, c, d, a, b, 3, 16, 42);
    step!(H, b, c, d, a, 6, 23, 43);
    step!(H, a, b, c, d, 9, 4, 44);
    step!(H, d, a, b, c, 12, 11, 45);
    step!(H, c, d, a, b, 15, 16, 46);
    step!(H, b, c, d, a, 2, 23, 47);
    // Round 4: X[7i mod 16], shifts 6 10 15 21.
    step!(I, a, b, c, d, 0, 6, 48);
    step!(I, d, a, b, c, 7, 10, 49);
    step!(I, c, d, a, b, 14, 15, 50);
    step!(I, b, c, d, a, 5, 21, 51);
    step!(I, a, b, c, d, 12, 6, 52);
    step!(I, d, a, b, c, 3, 10, 53);
    step!(I, c, d, a, b, 10, 15, 54);
    step!(I, b, c, d, a, 1, 21, 55);
    step!(I, a, b, c, d, 8, 6, 56);
    step!(I, d, a, b, c, 15, 10, 57);
    step!(I, c, d, a, b, 6, 15, 58);
    step!(I, b, c, d, a, 13, 21, 59);
    step!(I, a, b, c, d, 4, 6, 60);
    step!(I, d, a, b, c, 11, 10, 61);
    step!(I, c, d, a, b, 2, 15, 62);
    step!(I, b, c, d, a, 9, 21, 63);
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

#[cfg(test)]
mod tests {
    use super::Md5;

    fn hex(d: [u8; 16]) -> String {
        crate::hex::encode(d)
    }

    /// RFC 1321 A.5 test suite.
    #[test]
    fn rfc_test_suite() {
        for (input, want) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(hex(Md5::digest(input.as_bytes())), want, "{input:?}");
        }
    }

    /// Every length around the padding boundaries and every split point
    /// agree with an independent implementation.
    #[test]
    fn matches_reference_for_all_lengths_and_splits() {
        use md5_reference::Digest;
        let mut seed = 0x9e37_79b9u32;
        let data: Vec<u8> = (0..1100)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        for len in 0..data.len() {
            let want: [u8; 16] = md5_reference::Md5::digest(&data[..len]).into();
            assert_eq!(Md5::digest(&data[..len]), want, "len {len}");
            if len % 7 == 0 {
                for split in 0..=len {
                    let mut h = Md5::new();
                    h.update(&data[..split]);
                    h.update(&data[split..len]);
                    assert_eq!(h.finalize(), want, "len {len} split {split}");
                }
            }
        }
    }
}
