//! Deterministic pseudo-random number generator (PRNG) for RandSeed games.
//!
//! Implements 32-bit Mulberry32 with the JavaScript seed conversion required
//! by the shared room-host protocol.

/// Deterministic 32-bit Mulberry32 pseudo-random number generator.
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
