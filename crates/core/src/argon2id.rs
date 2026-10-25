//! Argon2id (RFC 9106, version 0x13) with a NEON block function for ARM64.
//!
//! The `argon2` crate has a hand-enabled AVX2 build of its block function on
//! x86-64 but runs it as scalar code on ARM64: the BlaMka mixing (64-bit adds,
//! 32x32 multiplies and rotations) is not vectorized there. This module computes
//! the same function with the mixing on two 64-bit lanes per NEON register.
//! [`crate::auth`] uses it on ARM64 for the 32-byte tags every stored hash
//! has; elsewhere, and for any other parameters, the crate is used. The tests
//! compare tags with the crate for many parameter sets.
use blake2::{Blake2b256, Blake2b512, Digest};

const BLOCK_WORDS: usize = 128;

/// One 1 KiB memory block, cache-line aligned.
#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(align(64))]
struct Block([u64; BLOCK_WORDS]);

impl std::ops::Deref for Block {
    type Target = [u64; BLOCK_WORDS];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Block {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

const ZERO: Block = Block([0; BLOCK_WORDS]);
const SYNC_POINTS: u32 = 4;
const ARGON2ID: u32 = 2;
const VERSION: u32 = 0x13;
/// Tag length this module computes (the `argon2` crate default).
pub const TAG_LEN: usize = 32;

/// Argon2id tag of `password` and `salt`: `m_kib` KiB of memory, `t` passes,
/// `p` lanes. `None` for parameters outside RFC 9106's limits.
pub fn hash(password: &[u8], salt: &[u8], m_kib: u32, t: u32, p: u32) -> Option<[u8; TAG_LEN]> {
    if !(1..=0x00ff_ffff).contains(&p) || t == 0 || m_kib < 8 * p || salt.len() < 8 {
        return None;
    }
    // H0 (RFC 9106 3.2 step 1); no secret, no associated data.
    let mut h = Blake2b512::new();
    for v in [p, TAG_LEN as u32, m_kib, t, VERSION, ARGON2ID] {
        h.update(v.to_le_bytes());
    }
    h.update((password.len() as u32).to_le_bytes());
    h.update(password);
    h.update((salt.len() as u32).to_le_bytes());
    h.update(salt);
    h.update(0u32.to_le_bytes());
    h.update(0u32.to_le_bytes());
    let h0: [u8; 64] = h.finalize().into();

    let lanes = p as usize;
    let blocks = (m_kib / (SYNC_POINTS * p) * (SYNC_POINTS * p)) as usize;
    let lane_len = blocks / lanes;
    let segment_len = lane_len / SYNC_POINTS as usize;
    let mut memory: Vec<Block> = vec![ZERO; blocks];

    // First two blocks of each lane: H'^1024(H0 || LE32(j) || LE32(lane)).
    for lane in 0..lanes {
        for j in 0..2u32 {
            let mut bytes = [0u8; 1024];
            h_prime_1024(&h0, j, lane as u32, &mut bytes);
            let block = &mut memory[lane * lane_len + j as usize];
            for (w, b) in block.iter_mut().zip(bytes.as_chunks::<8>().0) {
                *w = u64::from_le_bytes(*b);
            }
        }
    }

    for pass in 0..t {
        for slice in 0..SYNC_POINTS {
            for lane in 0..lanes {
                fill_segment(
                    &mut memory,
                    pass,
                    slice,
                    lane,
                    lanes,
                    lane_len,
                    segment_len,
                    blocks,
                    t,
                );
            }
        }
    }

    // Final block: XOR of each lane's last block, then H'^32.
    let mut last = memory[lane_len - 1];
    for lane in 1..lanes {
        let b = &memory[lane * lane_len + lane_len - 1];
        for (x, y) in last.iter_mut().zip(b.iter()) {
            *x ^= y;
        }
    }
    let mut bytes = [0u8; 1024];
    for (o, w) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(last.0) {
        *o = w.to_le_bytes();
    }
    let mut h = Blake2b256::new();
    h.update((TAG_LEN as u32).to_le_bytes());
    h.update(bytes);
    Some(h.finalize().into())
}

/// H'^1024 (RFC 9106 3.3): 30 chained BLAKE2b-512 outputs (first halves),
/// then a full final 64 bytes.
fn h_prime_1024(h0: &[u8; 64], j: u32, lane: u32, out: &mut [u8; 1024]) {
    let mut h = Blake2b512::new();
    h.update(1024u32.to_le_bytes());
    h.update(h0);
    h.update(j.to_le_bytes());
    h.update(lane.to_le_bytes());
    let mut v: [u8; 64] = h.finalize().into();
    out[..32].copy_from_slice(&v[..32]);
    for i in 1..30 {
        v = Blake2b512::digest(v).into();
        out[32 * i..32 * i + 32].copy_from_slice(&v[..32]);
    }
    v = Blake2b512::digest(v).into();
    out[960..].copy_from_slice(&v);
}

#[allow(clippy::too_many_arguments)]
fn fill_segment(
    memory: &mut [Block],
    pass: u32,
    slice: u32,
    lane: usize,
    lanes: usize,
    lane_len: usize,
    segment_len: usize,
    blocks: usize,
    passes: u32,
) {
    // Argon2id: data-independent addresses in the first half of the first pass.
    let independent = pass == 0 && slice < 2;
    let mut input = ZERO;
    let mut addresses = ZERO;
    if independent {
        input[..6].copy_from_slice(&[
            u64::from(pass),
            lane as u64,
            u64::from(slice),
            blocks as u64,
            u64::from(passes),
            u64::from(ARGON2ID),
        ]);
    }
    let next_addresses = |input: &mut Block, addresses: &mut Block| {
        input[6] += 1;
        let once = compress(&ZERO, input);
        *addresses = compress(&ZERO, &once);
    };
    let start = if pass == 0 && slice == 0 {
        if independent {
            next_addresses(&mut input, &mut addresses);
        }
        2
    } else {
        0
    };
    for index in start..segment_len {
        let column = slice as usize * segment_len + index;
        let current = lane * lane_len + column;
        let previous = if column == 0 {
            current + lane_len - 1
        } else {
            current - 1
        };
        let random = if independent {
            if index % BLOCK_WORDS == 0 {
                next_addresses(&mut input, &mut addresses);
            }
            addresses[index % BLOCK_WORDS]
        } else {
            memory[previous][0]
        };
        let (j1, j2) = (random & 0xffff_ffff, random >> 32);
        let ref_lane = if pass == 0 && slice == 0 {
            lane
        } else {
            (j2 % lanes as u64) as usize
        };
        let same_lane = ref_lane == lane;
        // Blocks this one may reference (RFC 9106 3.4.2).
        let area = if pass == 0 {
            if slice == 0 || same_lane {
                column - 1
            } else {
                slice as usize * segment_len - usize::from(index == 0)
            }
        } else if same_lane {
            lane_len - segment_len + index - 1
        } else {
            lane_len - segment_len - usize::from(index == 0)
        } as u64;
        let x = (j1 * j1) >> 32;
        let relative = area - 1 - ((area * x) >> 32);
        let first = if pass == 0 || slice == SYNC_POINTS - 1 {
            0
        } else {
            (slice as usize + 1) * segment_len
        };
        let ref_index = ref_lane * lane_len + (first + relative as usize) % lane_len;
        let r = xor(&memory[previous], &memory[ref_index]);
        // Version 0x13: from the second pass on, XOR into the old block.
        let out = if pass == 0 {
            r
        } else {
            xor(&r, &memory[current])
        };
        memory[current] = mix(&r, out);
    }
}

fn xor(x: &Block, y: &Block) -> Block {
    Block(std::array::from_fn(|i| x[i] ^ y[i]))
}

/// P applied to `r`'s rows then columns, XORed with `out`.
fn mix(r: &Block, mut out: Block) -> Block {
    let mut q = *r;
    permute_block(&mut q);
    for (o, q) in out.iter_mut().zip(q.0) {
        *o ^= q;
    }
    out
}

/// G(X, Y) (RFC 9106 3.5): R = X ^ Y, permute R's rows then columns, ^ R.
fn compress(x: &Block, y: &Block) -> Block {
    let r = xor(x, y);
    mix(&r, r)
}

/// Rows are 16 consecutive words; column c is words 2c, 2c+1, 2c+16, ...
/// Both are eight pairs `base + k * stride + {0, 1}`.
#[cfg(not(target_arch = "aarch64"))]
fn permute_block(q: &mut Block) {
    for row in 0..8 {
        permute::<2>(q, 16 * row);
    }
    for col in 0..8 {
        permute::<16>(q, 2 * col);
    }
}

#[cfg(any(test, not(target_arch = "aarch64")))]
#[inline(always)]
fn permute<const STRIDE: usize>(q: &mut Block, base: usize) {
    let words = &mut q[base..base + 7 * STRIDE + 2];
    let mut v: [u64; 16] = std::array::from_fn(|i| words[(i / 2) * STRIDE + i % 2]);
    fn bla(x: u64, y: u64) -> u64 {
        let p = (x & 0xffff_ffff) * (y & 0xffff_ffff);
        x.wrapping_add(y).wrapping_add(p.wrapping_mul(2))
    }
    macro_rules! g {
        ($a:expr, $b:expr, $c:expr, $d:expr) => {
            v[$a] = bla(v[$a], v[$b]);
            v[$d] = (v[$d] ^ v[$a]).rotate_right(32);
            v[$c] = bla(v[$c], v[$d]);
            v[$b] = (v[$b] ^ v[$c]).rotate_right(24);
            v[$a] = bla(v[$a], v[$b]);
            v[$d] = (v[$d] ^ v[$a]).rotate_right(16);
            v[$c] = bla(v[$c], v[$d]);
            v[$b] = (v[$b] ^ v[$c]).rotate_right(63);
        };
    }
    g!(0, 4, 8, 12);
    g!(1, 5, 9, 13);
    g!(2, 6, 10, 14);
    g!(3, 7, 11, 15);
    g!(0, 5, 10, 15);
    g!(1, 6, 11, 12);
    g!(2, 7, 8, 13);
    g!(3, 4, 9, 14);
    for (i, w) in v.into_iter().enumerate() {
        words[(i / 2) * STRIDE + i % 2] = w;
    }
}

/// The permutation with two 64-bit lanes per register. Each P works on 16
/// words as eight pairs (A0 A1 B0 B1 C0 C1 D0 D1); a row's pairs are
/// consecutive words, a column's pairs are 16 words apart.
#[cfg(target_arch = "aarch64")]
fn permute_block(q: &mut Block) {
    for row in 0..8 {
        neon::permute::<2>(q, 16 * row);
    }
    for col in 0..8 {
        neon::permute::<16>(q, 2 * col);
    }
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::Block;
    use core::arch::aarch64::*;

    /// a + b + 2 * lo32(a) * lo32(b), per 64-bit lane.
    #[inline(always)]
    fn bla(a: uint64x2_t, b: uint64x2_t) -> uint64x2_t {
        // SAFETY: NEON is part of the AArch64 baseline.
        unsafe {
            let p = vmull_u32(vmovn_u64(a), vmovn_u64(b));
            vaddq_u64(vaddq_u64(a, b), vaddq_u64(p, p))
        }
    }

    #[inline(always)]
    fn ror32(x: uint64x2_t) -> uint64x2_t {
        // SAFETY: NEON baseline; reinterpretations only.
        unsafe { vreinterpretq_u64_u32(vrev64q_u32(vreinterpretq_u32_u64(x))) }
    }

    #[inline(always)]
    fn ror_bytes(x: uint64x2_t, table: &[u8; 16]) -> uint64x2_t {
        // SAFETY: NEON baseline; `table` is 16 readable bytes.
        unsafe {
            vreinterpretq_u64_u8(vqtbl1q_u8(
                vreinterpretq_u8_u64(x),
                vld1q_u8(table.as_ptr()),
            ))
        }
    }

    /// Byte k of each 64-bit lane takes byte (k + n) mod 8: rotate right 8n bits.
    const ROR24: [u8; 16] = [3, 4, 5, 6, 7, 0, 1, 2, 11, 12, 13, 14, 15, 8, 9, 10];
    const ROR16: [u8; 16] = [2, 3, 4, 5, 6, 7, 0, 1, 10, 11, 12, 13, 14, 15, 8, 9];

    #[inline(always)]
    fn ror63(x: uint64x2_t) -> uint64x2_t {
        // SAFETY: NEON baseline. (x << 1) | (x >> 63).
        unsafe { vsriq_n_u64::<63>(vshlq_n_u64::<1>(x), x) }
    }

    #[inline(always)]
    fn g(a: &mut uint64x2_t, b: &mut uint64x2_t, c: &mut uint64x2_t, d: &mut uint64x2_t) {
        // SAFETY: NEON baseline.
        unsafe {
            *a = bla(*a, *b);
            *d = ror32(veorq_u64(*d, *a));
            *c = bla(*c, *d);
            *b = ror_bytes(veorq_u64(*b, *c), &ROR24);
            *a = bla(*a, *b);
            *d = ror_bytes(veorq_u64(*d, *a), &ROR16);
            *c = bla(*c, *d);
            *b = ror63(veorq_u64(*b, *c));
        }
    }

    /// P on the 16 words at `base + k * stride + {0, 1}`, k = 0..8.
    #[inline(always)]
    pub(super) fn permute<const STRIDE: usize>(q: &mut Block, base: usize) {
        let at = |k: usize| base + k * STRIDE;
        assert!(at(7) + 1 < q.len());
        // SAFETY: every index below is at most at(7) + 1, checked above;
        // each load/store touches two consecutive u64.
        unsafe {
            let p = q.as_mut_ptr();
            let [
                mut a0,
                mut a1,
                mut b0,
                mut b1,
                mut c0,
                mut c1,
                mut d0,
                mut d1,
            ] = core::array::from_fn(|k| vld1q_u64(p.add(at(k))));
            // Columns of the 4x4 state: (v0 v4 v8 v12) ... (v3 v7 v11 v15).
            g(&mut a0, &mut b0, &mut c0, &mut d0);
            g(&mut a1, &mut b1, &mut c1, &mut d1);
            // Diagonals: (v0 v5 v10 v15) (v1 v6 v11 v12) (v2 v7 v8 v13) (v3 v4 v9 v14).
            let (mut e0, mut e1) = (vextq_u64::<1>(b0, b1), vextq_u64::<1>(b1, b0));
            let (mut f0, mut f1) = (vextq_u64::<1>(d1, d0), vextq_u64::<1>(d0, d1));
            g(&mut a0, &mut e0, &mut c1, &mut f0);
            g(&mut a1, &mut e1, &mut c0, &mut f1);
            (b0, b1) = (vextq_u64::<1>(e1, e0), vextq_u64::<1>(e0, e1));
            (d0, d1) = (vextq_u64::<1>(f0, f1), vextq_u64::<1>(f1, f0));
            for (k, v) in [a0, a1, b0, b1, c0, c1, d0, d1].into_iter().enumerate() {
                vst1q_u64(p.add(at(k)), v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::{Algorithm, Argon2, Params, Version};

    fn reference(password: &[u8], salt: &[u8], m: u32, t: u32, p: u32) -> [u8; TAG_LEN] {
        let mut out = [0u8; TAG_LEN];
        Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(m, t, p, Some(TAG_LEN)).unwrap(),
        )
        .hash_password_into(password, salt, &mut out)
        .unwrap();
        out
    }

    /// RFC 9106 5.3 parameters (without secret/associated data, which the
    /// RFC vector uses) against the crate, plus the production profile and
    /// lane counts, pass counts and memory sizes around the segment limits.
    #[test]
    fn matches_the_argon2_crate() {
        let cases: &[(u32, u32, u32)] = &[
            (32, 3, 4),
            (8, 1, 1),
            (16, 1, 2),
            (64, 1, 1),
            (64, 2, 1),
            (100, 3, 3),
            (512, 1, 1),
            (1024, 2, 4),
            (19 * 1024, 2, 1),
        ];
        for &(m, t, p) in cases {
            for (password, salt) in [
                (&b"password"[..], &b"somesaltsomesalt"[..]),
                (b"", b"saltsalt"),
                (
                    "비밀번호-Long-test-password-123!".as_bytes(),
                    b"0123456789abcdef0123456789abcdef",
                ),
            ] {
                assert_eq!(
                    hash(password, salt, m, t, p).unwrap(),
                    reference(password, salt, m, t, p),
                    "m={m} t={t} p={p}"
                );
            }
        }
    }

    /// The NEON permutation (ARM64) and the portable one agree.
    #[test]
    fn permutation_matches_portable() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut block = ZERO;
        for w in block.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *w = seed;
        }
        let mut portable = block;
        for row in 0..8 {
            permute::<2>(&mut portable, 16 * row);
        }
        for col in 0..8 {
            permute::<16>(&mut portable, 2 * col);
        }
        permute_block(&mut block);
        assert_eq!(block, portable);
    }

    #[test]
    fn rejects_out_of_range_parameters() {
        assert!(hash(b"pw", b"saltsalt", 7, 1, 1).is_none());
        assert!(hash(b"pw", b"saltsalt", 64, 0, 1).is_none());
        assert!(hash(b"pw", b"short", 64, 1, 1).is_none());
        assert!(hash(b"pw", b"saltsalt", 64, 1, 0).is_none());
    }
}
