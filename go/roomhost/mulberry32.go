package roomhost

import "math"

const uint32Range = 4294967296.0

// ToInt32 converts a float64 seed using JavaScript's signed 32-bit conversion.
func ToInt32(seed float64) int32 {
	if math.IsNaN(seed) || math.IsInf(seed, 0) || seed == 0 {
		return 0
	}
	value := math.Mod(math.Trunc(seed), uint32Range)
	if value < 0 {
		value += uint32Range
	}
	return int32(uint32(value))
}

// Mulberry32 is a deterministic, non-cryptographic 32-bit pseudorandom generator.
type Mulberry32 struct {
	state uint32
}

// NewMulberry32 creates a generator using JavaScript-compatible ToInt32 seed handling.
func NewMulberry32(seed float64) *Mulberry32 {
	return &Mulberry32{state: uint32(ToInt32(seed))}
}

// NextU32 returns the next unsigned 32-bit output.
func (generator *Mulberry32) NextU32() uint32 {
	generator.state += 0x6D2B79F5
	value := generator.state
	value = (value ^ (value >> 15)) * (value | 1)
	value ^= value + (value^(value>>7))*(value|61)
	return value ^ (value >> 14)
}

// NextFloat returns the next output divided by 2^32, in the range [0, 1).
func (generator *Mulberry32) NextFloat() float64 {
	return float64(generator.NextU32()) / uint32Range
}
