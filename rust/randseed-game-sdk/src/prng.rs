//! Deterministic pseudo-random number generators (PRNG) and domain-separated
//! seed derivation for RandSeed games.
//!
//! - [`ChaCha20Rng`] implements 20-round ChaCha20 matching `rand_chacha::ChaCha20Rng`
//!   (256-bit root seed from `options.random_seed`, 64-bit little-endian block counter,
//!   64-bit little-endian stream ID, and 32-bit little-endian word cursor).
//! - [`derive_seed`] derives collision-free 256-bit sub-seeds using length-prefixed
//!   SHA-256 domain separation (`b"randseed-chacha20-derive-v1\0"`).
//! - [`Mulberry32`] is retained for legacy compatibility with `options.seed`.

use crate::errors::{SdkError, SdkErrorCode};
use serde::{Deserialize, Serialize};

/// Byte length of a canonical RandSeed match root seed (`[u8; 32]`).
pub const RANDOM_SEED_BYTES: usize = 32;

/// Character length of a hex-encoded RandSeed match root seed (`64` lowercase hex chars).
pub const RANDOM_SEED_HEX_LEN: usize = 64;

/// Domain-separation prefix used by [`derive_seed`].
pub const DERIVE_SEED_DOMAIN_PREFIX: &[u8] = b"randseed-chacha20-derive-v1\0";

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const CHACHA20_CONSTANTS: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];
const TWO_POW_32: u64 = 0x1_0000_0000;

/// Check whether `hex` is a canonical 64-character lowercase ASCII hex seed string.
pub fn is_valid_seed_hex(hex: &str) -> bool {
    hex.len() == RANDOM_SEED_HEX_LEN
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Parse a canonical 64-character lowercase hex string into a 32-byte seed.
pub fn parse_seed_hex(hex: &str) -> Result<[u8; RANDOM_SEED_BYTES], SdkError> {
    if hex.len() != RANDOM_SEED_HEX_LEN {
        return Err(SdkError::protocol_with_message(
            SdkErrorCode::InvalidOptions,
            "randomSeed must be a 64-character lowercase hex string.",
        ));
    }
    let bytes = hex.as_bytes();
    let mut out = [0u8; RANDOM_SEED_BYTES];
    for (idx, slot) in out.iter_mut().enumerate() {
        let Some(hi) = hex_nibble(bytes[idx * 2]) else {
            return Err(SdkError::protocol_with_message(
                SdkErrorCode::InvalidOptions,
                "randomSeed must be a 64-character lowercase hex string.",
            ));
        };
        let Some(lo) = hex_nibble(bytes[idx * 2 + 1]) else {
            return Err(SdkError::protocol_with_message(
                SdkErrorCode::InvalidOptions,
                "randomSeed must be a 64-character lowercase hex string.",
            ));
        };
        *slot = (hi << 4) | lo;
    }
    Ok(out)
}

/// Encode a 32-byte seed as a 64-character lowercase hex string.
pub fn encode_seed_hex(seed: &[u8; RANDOM_SEED_BYTES]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(RANDOM_SEED_HEX_LEN);
    for &b in seed {
        out.push(HEX_CHARS[(b >> 4) as usize] as char);
        out.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    out
}

/// Extract the deterministic legacy 32-bit unsigned integer (`options.seed`) from a 32-byte root seed.
pub fn legacy_seed_from_bytes(seed: &[u8; RANDOM_SEED_BYTES]) -> u32 {
    u32::from_le_bytes([seed[0], seed[1], seed[2], seed[3]])
}

/// Derive a deterministic 32-byte sub-seed from `root_seed` and length-prefixed `domains` using SHA-256.
///
/// Preimage layout:
/// `b"randseed-chacha20-derive-v1\0" || root_seed || u32_le(domains.len()) || concat(u32_le(d.len()) || d)`.
pub fn derive_seed(
    root_seed: &[u8; RANDOM_SEED_BYTES],
    domains: &[&[u8]],
) -> [u8; RANDOM_SEED_BYTES] {
    let mut hasher = Sha256::new();
    hasher.update(DERIVE_SEED_DOMAIN_PREFIX);
    hasher.update(root_seed);
    let count = u32::try_from(domains.len()).expect("domain count fits in u32");
    hasher.update(&count.to_le_bytes());
    for domain in domains {
        let len = u32::try_from(domain.len()).expect("domain byte length fits in u32");
        hasher.update(&len.to_le_bytes());
        hasher.update(domain);
    }
    hasher.finalize()
}

/// Derive a deterministic 32-byte sub-seed using UTF-8 string domain labels.
pub fn derive_seed_strs(
    root_seed: &[u8; RANDOM_SEED_BYTES],
    domains: &[&str],
) -> [u8; RANDOM_SEED_BYTES] {
    let mut hasher = Sha256::new();
    hasher.update(DERIVE_SEED_DOMAIN_PREFIX);
    hasher.update(root_seed);
    let count = u32::try_from(domains.len()).expect("domain count fits in u32");
    hasher.update(&count.to_le_bytes());
    for domain in domains {
        let bytes = domain.as_bytes();
        let len = u32::try_from(bytes.len()).expect("domain byte length fits in u32");
        hasher.update(&len.to_le_bytes());
        hasher.update(bytes);
    }
    hasher.finalize()
}

/// Derive a deterministic 64-character lowercase hex sub-seed from a hex root seed and string domains.
pub fn derive_seed_hex(root_seed_hex: &str, domains: &[&str]) -> Result<String, SdkError> {
    let root = parse_seed_hex(root_seed_hex)?;
    Ok(encode_seed_hex(&derive_seed_strs(&root, domains)))
}

/// Serializable cursor snapshot for [`ChaCha20Rng`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChaCha20State {
    /// Canonical 64-character lowercase hex representation of the 32-byte key.
    pub seed_hex: String,
    /// 64-bit ChaCha20 stream identifier (non-negative safe integer on JSON wire).
    pub stream: u64,
    /// Number of 32-bit words already consumed from this `(seed, stream)` sequence.
    pub word_pos: u64,
}

impl ChaCha20State {
    /// Validate that the state uses canonical hex and safe JSON integer bounds.
    pub fn validate(&self) -> Result<(), SdkError> {
        if !is_valid_seed_hex(&self.seed_hex) {
            return Err(SdkError::Validation(
                "ChaCha20State.seedHex must be a 64-character lowercase hex string.".into(),
            ));
        }
        if self.stream > MAX_SAFE_INTEGER {
            return Err(SdkError::Validation(
                "ChaCha20State.stream must be a non-negative safe integer.".into(),
            ));
        }
        if self.word_pos > MAX_SAFE_INTEGER {
            return Err(SdkError::Validation(
                "ChaCha20State.wordPos must be a non-negative safe integer.".into(),
            ));
        }
        Ok(())
    }
}

/// Deterministic 20-round ChaCha20 pseudo-random number generator.
///
/// Bit-for-bit compatible with `rand_chacha::ChaCha20Rng` across all RandSeed SDK languages.
#[derive(Debug, Clone)]
pub struct ChaCha20Rng {
    seed: [u8; RANDOM_SEED_BYTES],
    key_words: [u32; 8],
    stream: u64,
    word_pos: u64,
    cached_block_index: Option<u64>,
    cached_block: [u32; 16],
}

impl PartialEq for ChaCha20Rng {
    fn eq(&self, other: &Self) -> bool {
        self.seed == other.seed && self.stream == other.stream && self.word_pos == other.word_pos
    }
}

impl Eq for ChaCha20Rng {}

impl ChaCha20Rng {
    /// Create a generator from a 32-byte seed at `stream = 0` and `word_pos = 0`.
    pub fn from_seed(seed: [u8; RANDOM_SEED_BYTES]) -> Self {
        let mut key_words = [0u32; 8];
        for (idx, word) in key_words.iter_mut().enumerate() {
            let offset = idx * 4;
            *word = u32::from_le_bytes([
                seed[offset],
                seed[offset + 1],
                seed[offset + 2],
                seed[offset + 3],
            ]);
        }
        Self {
            seed,
            key_words,
            stream: 0,
            word_pos: 0,
            cached_block_index: None,
            cached_block: [0u32; 16],
        }
    }

    /// Create a generator from a canonical 64-character lowercase hex string.
    pub fn from_seed_hex(seed_hex: &str) -> Result<Self, SdkError> {
        Ok(Self::from_seed(parse_seed_hex(seed_hex)?))
    }

    /// Create a generator by deriving a sub-seed from `root_seed` and byte-slice `domains`.
    pub fn from_derived(root_seed: &[u8; RANDOM_SEED_BYTES], domains: &[&[u8]]) -> Self {
        Self::from_seed(derive_seed(root_seed, domains))
    }

    /// Create a generator by deriving a sub-seed from `root_seed` and UTF-8 string `domains`.
    pub fn from_derived_strs(root_seed: &[u8; RANDOM_SEED_BYTES], domains: &[&str]) -> Self {
        Self::from_seed(derive_seed_strs(root_seed, domains))
    }

    /// Restore a generator from a serialized [`ChaCha20State`].
    pub fn from_state(state: &ChaCha20State) -> Result<Self, SdkError> {
        state.validate()?;
        let mut rng = Self::from_seed(parse_seed_hex(&state.seed_hex)?);
        rng.stream = state.stream;
        rng.word_pos = state.word_pos;
        Ok(rng)
    }

    /// Export a serializable [`ChaCha20State`] cursor snapshot.
    pub fn state(&self) -> ChaCha20State {
        ChaCha20State {
            seed_hex: self.seed_hex(),
            stream: self.stream,
            word_pos: self.word_pos,
        }
    }

    /// Return a copy of this generator configured with the given 64-bit `stream` ID.
    pub fn with_stream(mut self, stream: u64) -> Self {
        self.set_stream(stream);
        self
    }

    /// Set the 64-bit ChaCha20 stream ID and reset `word_pos` to `0`? No: preserves `word_pos`
    /// unless caller calls `set_word_pos(0)`. Note: `rand_chacha::set_stream` resets/keeps block pos;
    /// we invalidate block cache so subsequent reads use the new stream at current `word_pos`.
    pub fn set_stream(&mut self, stream: u64) {
        if self.stream != stream {
            self.stream = stream;
            self.cached_block_index = None;
        }
    }

    /// Return the current 64-bit ChaCha20 stream ID.
    pub fn stream(&self) -> u64 {
        self.stream
    }

    /// Seek to an exact 32-bit word offset (`word_pos`) within the current stream.
    pub fn set_word_pos(&mut self, word_pos: u64) {
        self.word_pos = word_pos;
    }

    /// Return the current 32-bit word offset (`word_pos`).
    pub fn word_pos(&self) -> u64 {
        self.word_pos
    }

    /// Return the underlying 32-byte key.
    pub fn seed(&self) -> &[u8; RANDOM_SEED_BYTES] {
        &self.seed
    }

    /// Return the canonical 64-character lowercase hex representation of the key.
    pub fn seed_hex(&self) -> String {
        encode_seed_hex(&self.seed)
    }

    /// Generate the next unsigned 32-bit integer and advance `word_pos` by 1.
    pub fn next_u32(&mut self) -> u32 {
        let block_index = self.word_pos >> 4;
        let word_offset = (self.word_pos & 15) as usize;
        if self.cached_block_index != Some(block_index) {
            self.cached_block = chacha20_block(&self.key_words, block_index, self.stream);
            self.cached_block_index = Some(block_index);
        }
        self.word_pos = self
            .word_pos
            .checked_add(1)
            .expect("ChaCha20Rng word_pos overflow");
        self.cached_block[word_offset]
    }

    /// Generate the next unsigned 64-bit integer from two consecutive little-endian `u32` words.
    pub fn next_u64(&mut self) -> u64 {
        let lo = u64::from(self.next_u32());
        let hi = u64::from(self.next_u32());
        lo | (hi << 32)
    }

    /// Generate the next `f64` value in the half-open range `[0.0, 1.0)` (`next_u32() / 2^32`).
    pub fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4_294_967_296.0
    }

    /// Fill `dest` with little-endian bytes drawn from consecutive `next_u32()` words.
    pub fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(4) {
            let bytes = self.next_u32().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }

    /// Generate an unbiased integer in `0..bound` using canonical rejection sampling (`bound > 0`).
    pub fn next_bounded(&mut self, bound: u32) -> u32 {
        assert!(bound > 0, "ChaCha20Rng::next_bounded requires bound > 0");
        let bound_u64 = u64::from(bound);
        let limit = TWO_POW_32 - (TWO_POW_32 % bound_u64);
        loop {
            let candidate = u64::from(self.next_u32());
            if candidate < limit {
                return (candidate % bound_u64) as u32;
            }
        }
    }

    /// Generate an unbiased signed 32-bit integer in the inclusive range `min..=max_inclusive`.
    pub fn next_range_i32(&mut self, min: i32, max_inclusive: i32) -> i32 {
        assert!(
            min <= max_inclusive,
            "ChaCha20Rng::next_range_i32 requires min <= max_inclusive"
        );
        let span = (i64::from(max_inclusive) - i64::from(min)) as u64;
        if span == u64::from(u32::MAX) {
            return (i64::from(min) + i64::from(self.next_u32())) as i32;
        }
        let offset = self.next_bounded((span + 1) as u32);
        (i64::from(min) + i64::from(offset)) as i32
    }

    /// Generate an unbiased boolean value.
    pub fn next_bool(&mut self) -> bool {
        self.next_bounded(2) == 1
    }

    /// Deterministically shuffle `slice` in-place using Fisher-Yates (`i` from `len - 1` down to `1`).
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        if slice.len() <= 1 {
            return;
        }
        for i in (1..slice.len()).rev() {
            let j = self.next_bounded((i + 1) as u32) as usize;
            slice.swap(i, j);
        }
    }
}

#[inline]
fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);

    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

fn chacha20_block(key_words: &[u32; 8], block_counter: u64, stream: u64) -> [u32; 16] {
    let mut initial = [0u32; 16];
    initial[0..4].copy_from_slice(&CHACHA20_CONSTANTS);
    initial[4..12].copy_from_slice(key_words);
    initial[12] = (block_counter & 0xffff_ffff) as u32;
    initial[13] = (block_counter >> 32) as u32;
    initial[14] = (stream & 0xffff_ffff) as u32;
    initial[15] = (stream >> 32) as u32;

    let mut working = initial;
    for _ in 0..10 {
        quarter_round(&mut working, 0, 4, 8, 12);
        quarter_round(&mut working, 1, 5, 9, 13);
        quarter_round(&mut working, 2, 6, 10, 14);
        quarter_round(&mut working, 3, 7, 11, 15);

        quarter_round(&mut working, 0, 5, 10, 15);
        quarter_round(&mut working, 1, 6, 11, 12);
        quarter_round(&mut working, 2, 7, 8, 13);
        quarter_round(&mut working, 3, 4, 9, 14);
    }

    for (slot, init_word) in working.iter_mut().zip(initial.iter()) {
        *slot = slot.wrapping_add(*init_word);
    }
    working
}

// Minimal, self-contained FIPS 180-4 SHA-256 implementation for domain-separated seed derivation.
struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_bytes: u64,
}

const SHA256_INIT: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

const SHA256_K: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
    0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
    0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
    0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
    0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
    0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
    0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
    0xc671_78f2,
];

impl Sha256 {
    fn new() -> Self {
        Self {
            state: SHA256_INIT,
            buffer: [0u8; 64],
            buffer_len: 0,
            total_bytes: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_bytes = self
            .total_bytes
            .checked_add(data.len() as u64)
            .expect("SHA-256 input length overflow");

        if self.buffer_len > 0 {
            let take = (64 - self.buffer_len).min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&data[..take]);
            self.buffer_len += take;
            data = &data[take..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }

        while data.len() >= 64 {
            let (block, rest) = data.split_at(64);
            let mut arr = [0u8; 64];
            arr.copy_from_slice(block);
            self.compress(&arr);
            data = rest;
        }

        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffer_len = data.len();
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let total_bits = self.total_bytes * 8;
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..64].fill(0);
            let block = self.buffer;
            self.compress(&block);
            self.buffer_len = 0;
        }

        self.buffer[self.buffer_len..56].fill(0);
        self.buffer[56..64].copy_from_slice(&total_bits.to_be_bytes());
        let final_block = self.buffer;
        self.compress(&final_block);

        let mut out = [0u8; 32];
        for (idx, word) in self.state.iter().enumerate() {
            out[idx * 4..idx * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let offset = i * 4;
            w[i] = u32::from_be_bytes([
                block[offset],
                block[offset + 1],
                block[offset + 2],
                block[offset + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

/// Deterministic 32-bit Mulberry32 pseudo-random number generator.
///
/// Retained for legacy compatibility with `options.seed`. New games should use
/// [`ChaCha20Rng`] initialized from `options.random_seed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mulberry32 {
    a: u32,
}

impl Mulberry32 {
    /// Create a generator using JavaScript signed 32-bit seed conversion.
    pub fn new(seed: f64) -> Self {
        let seed = if seed.is_finite() {
            let remainder = seed.trunc() % 4_294_967_296.0;
            if remainder < 0.0 {
                remainder + 4_294_967_296.0
            } else {
                remainder
            }
        } else {
            0.0
        };
        Self { a: seed as u32 }
    }

    /// Create a generator from an unsigned 32-bit seed.
    pub fn from_u32(seed: u32) -> Self {
        Self { a: seed }
    }

    /// Create a generator from a signed 32-bit seed.
    pub fn from_i32(seed: i32) -> Self {
        Self { a: seed as u32 }
    }

    /// Generate the next unsigned 32-bit value.
    pub fn next_u32(&mut self) -> u32 {
        self.a = self.a.wrapping_add(0x6D2B79F5);
        let mut t = (self.a ^ (self.a >> 15)).wrapping_mul(1 | self.a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        t ^ (t >> 14)
    }

    /// Generate the next `f64` value in the half-open range `[0.0, 1.0)`.
    pub fn next_f64(&mut self) -> f64 {
        let u = self.next_u32();
        (u as f64) / 4294967296.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::{RngCore, SeedableRng};

    #[test]
    fn sha256_matches_nist_vectors() {
        let mut empty = Sha256::new();
        empty.update(b"");
        assert_eq!(
            encode_seed_hex(&empty.finalize()),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let mut abc = Sha256::new();
        abc.update(b"abc");
        assert_eq!(
            encode_seed_hex(&abc.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        // Multi-block 448-bit / 56-byte boundary test
        let mut multi = Sha256::new();
        multi.update(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(
            encode_seed_hex(&multi.finalize()),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn chacha20_matches_rand_chacha_across_blocks_streams_and_seeks() {
        for seed_byte in [0u8, 1u8, 0x5au8, 0xffu8] {
            let mut seed = [0u8; 32];
            for (i, slot) in seed.iter_mut().enumerate() {
                *slot = seed_byte.wrapping_add((i as u8).wrapping_mul(17));
            }

            let mut ours = ChaCha20Rng::from_seed(seed);
            let mut reference = rand_chacha::ChaCha20Rng::from_seed(seed);

            // Verify 100 consecutive words across multiple 16-word ChaCha20 blocks
            for word_idx in 0..100 {
                assert_eq!(
                    ours.next_u32(),
                    reference.next_u32(),
                    "mismatch at word {word_idx} for seed_byte={seed_byte}"
                );
            }
            assert_eq!(ours.next_u64(), reference.next_u64());

            // Verify non-zero stream parity
            let stream_id = 0x0102_0304_0506_0708u64;
            let mut ours_stream = ChaCha20Rng::from_seed(seed).with_stream(stream_id);
            let mut ref_stream = rand_chacha::ChaCha20Rng::from_seed(seed);
            ref_stream.set_stream(stream_id);
            for word_idx in 0..40 {
                assert_eq!(
                    ours_stream.next_u32(),
                    ref_stream.next_u32(),
                    "stream mismatch at word {word_idx}"
                );
            }

            // Verify state snapshot & seek restoration mid-block
            let mut seek_rng = ChaCha20Rng::from_seed(seed);
            for _ in 0..23 {
                let _ = seek_rng.next_u32();
            }
            let snap = seek_rng.state();
            let mut restored = ChaCha20Rng::from_state(&snap).unwrap();
            for _ in 0..40 {
                assert_eq!(seek_rng.next_u32(), restored.next_u32());
            }
        }
    }

    #[test]
    fn derive_seed_prevents_domain_concatenation_collisions() {
        let root = [7u8; 32];
        let a = derive_seed_strs(&root, &["ab", "c"]);
        let b = derive_seed_strs(&root, &["a", "bc"]);
        let empty = derive_seed_strs(&root, &[]);
        let single_empty = derive_seed_strs(&root, &[""]);
        assert_ne!(a, b);
        assert_ne!(empty, single_empty);
    }
}
