package roomhost

import (
	"crypto/sha256"
	"encoding/binary"
	"errors"
	"fmt"
	"math/bits"
)

const (
	// RandomSeedBytes is the byte length of a canonical RandSeed match root seed.
	RandomSeedBytes = 32
	// RandomSeedHexLen is the character length of a hex-encoded match root seed.
	RandomSeedHexLen = 64
	// DeriveSeedDomainPrefix is the SHA-256 domain-separation prefix for DeriveSeed.
	DeriveSeedDomainPrefix = "randseed-chacha20-derive-v1\x00"

	twoPow32 uint64 = 1 << 32
)

var chacha20Constants = [4]uint32{
	0x61707865,
	0x3320646e,
	0x79622d32,
	0x6b206574,
}

// ChaCha20State is a serializable cursor snapshot for ChaCha20Rng.
type ChaCha20State struct {
	SeedHex string `json:"seedHex"`
	Stream  uint64 `json:"stream"`
	WordPos uint64 `json:"wordPos"`
}

// IsValidSeedHex reports whether value is a 64-character lowercase ASCII hex string.
func IsValidSeedHex(value string) bool {
	if len(value) != RandomSeedHexLen {
		return false
	}
	for i := 0; i < len(value); i++ {
		b := value[i]
		if !((b >= '0' && b <= '9') || (b >= 'a' && b <= 'f')) {
			return false
		}
	}
	return true
}

func decodeHexNibble(b byte) (byte, bool) {
	switch {
	case b >= '0' && b <= '9':
		return b - '0', true
	case b >= 'a' && b <= 'f':
		return b - 'a' + 10, true
	default:
		return 0, false
	}
}

// ParseSeedHex decodes a canonical 64-character lowercase hex string into a 32-byte seed.
func ParseSeedHex(seedHex string) ([RandomSeedBytes]byte, error) {
	var out [RandomSeedBytes]byte
	if len(seedHex) != RandomSeedHexLen {
		return out, errors.New("randomSeed must be a 64-character lowercase hex string")
	}
	for i := 0; i < RandomSeedBytes; i++ {
		hi, okHi := decodeHexNibble(seedHex[i*2])
		lo, okLo := decodeHexNibble(seedHex[i*2+1])
		if !okHi || !okLo {
			return [RandomSeedBytes]byte{}, errors.New("randomSeed must be a 64-character lowercase hex string")
		}
		out[i] = (hi << 4) | lo
	}
	return out, nil
}

// EncodeSeedHex formats a 32-byte seed as a 64-character lowercase hex string.
func EncodeSeedHex(seed [RandomSeedBytes]byte) string {
	const hexChars = "0123456789abcdef"
	buf := make([]byte, RandomSeedHexLen)
	for i, b := range seed {
		buf[i*2] = hexChars[b>>4]
		buf[i*2+1] = hexChars[b&0x0f]
	}
	return string(buf)
}

// LegacySeedFromBytes extracts the deterministic 32-bit compatibility seed (options.seed) from a 32-byte root seed.
func LegacySeedFromBytes(seed [RandomSeedBytes]byte) uint32 {
	return binary.LittleEndian.Uint32(seed[0:4])
}

// DeriveSeed derives a collision-free 32-byte sub-seed from rootSeed and length-prefixed byte domains via SHA-256.
func DeriveSeed(rootSeed [RandomSeedBytes]byte, domains ...[]byte) [RandomSeedBytes]byte {
	hasher := sha256.New()
	hasher.Write([]byte(DeriveSeedDomainPrefix))
	hasher.Write(rootSeed[:])
	var lenBuf [4]byte
	binary.LittleEndian.PutUint32(lenBuf[:], uint32(len(domains)))
	hasher.Write(lenBuf[:])
	for _, domain := range domains {
		binary.LittleEndian.PutUint32(lenBuf[:], uint32(len(domain)))
		hasher.Write(lenBuf[:])
		hasher.Write(domain)
	}
	var out [RandomSeedBytes]byte
	copy(out[:], hasher.Sum(nil))
	return out
}

// DeriveSeedStrings derives a collision-free 32-byte sub-seed using UTF-8 string domain labels.
func DeriveSeedStrings(rootSeed [RandomSeedBytes]byte, domains ...string) [RandomSeedBytes]byte {
	byteDomains := make([][]byte, len(domains))
	for i, d := range domains {
		byteDomains[i] = []byte(d)
	}
	return DeriveSeed(rootSeed, byteDomains...)
}

// DeriveSeedHex derives a 64-character lowercase hex sub-seed from a hex root seed and string domains.
func DeriveSeedHex(rootSeedHex string, domains ...string) (string, error) {
	root, err := ParseSeedHex(rootSeedHex)
	if err != nil {
		return "", err
	}
	return EncodeSeedHex(DeriveSeedStrings(root, domains...)), nil
}

// ChaCha20Rng is a deterministic 20-round ChaCha20 PRNG bit-compatible with Rust rand_chacha::ChaCha20Rng.
type ChaCha20Rng struct {
	seed             [RandomSeedBytes]byte
	keyWords         [8]uint32
	stream           uint64
	wordPos          uint64
	hasCachedBlock   bool
	cachedBlockIndex uint64
	cachedBlock      [16]uint32
}

// NewChaCha20Rng creates a generator from a 32-byte seed at stream=0 and wordPos=0.
func NewChaCha20Rng(seed [RandomSeedBytes]byte) *ChaCha20Rng {
	var keyWords [8]uint32
	for i := 0; i < 8; i++ {
		keyWords[i] = binary.LittleEndian.Uint32(seed[i*4 : i*4+4])
	}
	return &ChaCha20Rng{
		seed:     seed,
		keyWords: keyWords,
	}
}

// NewChaCha20RngFromHex creates a generator from a canonical 64-character lowercase hex seed.
func NewChaCha20RngFromHex(seedHex string) (*ChaCha20Rng, error) {
	seed, err := ParseSeedHex(seedHex)
	if err != nil {
		return nil, err
	}
	return NewChaCha20Rng(seed), nil
}

// NewChaCha20RngDerived creates a generator by deriving a sub-seed from rootSeed and string domains.
func NewChaCha20RngDerived(rootSeed [RandomSeedBytes]byte, domains ...string) *ChaCha20Rng {
	return NewChaCha20Rng(DeriveSeedStrings(rootSeed, domains...))
}

// NewChaCha20RngFromState restores a generator from a serialized ChaCha20State.
func NewChaCha20RngFromState(state ChaCha20State) (*ChaCha20Rng, error) {
	if state.Stream > uint64(maxSafeInteger) || state.WordPos > uint64(maxSafeInteger) {
		return nil, fmt.Errorf("ChaCha20State stream and wordPos must be non-negative safe integers")
	}
	rng, err := NewChaCha20RngFromHex(state.SeedHex)
	if err != nil {
		return nil, err
	}
	rng.stream = state.Stream
	rng.wordPos = state.WordPos
	return rng, nil
}

// State exports a serializable cursor snapshot.
func (g *ChaCha20Rng) State() ChaCha20State {
	return ChaCha20State{
		SeedHex: EncodeSeedHex(g.seed),
		Stream:  g.stream,
		WordPos: g.wordPos,
	}
}

// SetStream sets the 64-bit ChaCha20 stream ID.
func (g *ChaCha20Rng) SetStream(stream uint64) {
	if g.stream != stream {
		g.stream = stream
		g.hasCachedBlock = false
	}
}

// Stream returns the current 64-bit ChaCha20 stream ID.
func (g *ChaCha20Rng) Stream() uint64 {
	return g.stream
}

// SetWordPos seeks to an exact 32-bit word offset within the current stream.
func (g *ChaCha20Rng) SetWordPos(wordPos uint64) {
	g.wordPos = wordPos
}

// WordPos returns the current 32-bit word offset.
func (g *ChaCha20Rng) WordPos() uint64 {
	return g.wordPos
}

// NextU32 returns the next unsigned 32-bit integer and advances wordPos by 1.
func (g *ChaCha20Rng) NextU32() uint32 {
	blockIndex := g.wordPos >> 4
	wordOffset := g.wordPos & 15
	if !g.hasCachedBlock || g.cachedBlockIndex != blockIndex {
		g.cachedBlock = chacha20Block(g.keyWords, blockIndex, g.stream)
		g.cachedBlockIndex = blockIndex
		g.hasCachedBlock = true
	}
	g.wordPos++
	return g.cachedBlock[wordOffset]
}

// NextU64 returns the next unsigned 64-bit integer from two consecutive little-endian u32 words.
func (g *ChaCha20Rng) NextU64() uint64 {
	lo := uint64(g.NextU32())
	hi := uint64(g.NextU32())
	return lo | (hi << 32)
}

// NextFloat returns NextU32() / 2^32 in [0, 1).
func (g *ChaCha20Rng) NextFloat() float64 {
	return float64(g.NextU32()) / uint32Range
}

// NextBounded returns an unbiased integer in [0, bound) using canonical rejection sampling (bound > 0).
func (g *ChaCha20Rng) NextBounded(bound uint32) uint32 {
	if bound == 0 {
		panic("roomhost: ChaCha20Rng.NextBounded requires bound > 0")
	}
	bound64 := uint64(bound)
	limit := twoPow32 - (twoPow32 % bound64)
	for {
		candidate := uint64(g.NextU32())
		if candidate < limit {
			return uint32(candidate % bound64)
		}
	}
}

// NextIntRange returns an unbiased int32 in [min, maxInclusive].
func (g *ChaCha20Rng) NextIntRange(minVal, maxInclusive int32) int32 {
	if minVal > maxInclusive {
		panic("roomhost: ChaCha20Rng.NextIntRange requires minVal <= maxInclusive")
	}
	span := uint64(int64(maxInclusive) - int64(minVal))
	if span == 0xffffffff {
		return int32(int64(minVal) + int64(g.NextU32()))
	}
	offset := g.NextBounded(uint32(span + 1))
	return int32(int64(minVal) + int64(offset))
}

// NextBool returns an unbiased boolean.
func (g *ChaCha20Rng) NextBool() bool {
	return g.NextBounded(2) == 1
}

// ShuffleSlice deterministically shuffles items in place using Fisher-Yates.
func ShuffleSlice[T any](g *ChaCha20Rng, items []T) {
	for i := len(items) - 1; i >= 1; i-- {
		j := int(g.NextBounded(uint32(i + 1)))
		items[i], items[j] = items[j], items[i]
	}
}

func quarterRound(state *[16]uint32, a, b, c, d int) {
	state[a] += state[b]
	state[d] = bits.RotateLeft32(state[d]^state[a], 16)

	state[c] += state[d]
	state[b] = bits.RotateLeft32(state[b]^state[c], 12)

	state[a] += state[b]
	state[d] = bits.RotateLeft32(state[d]^state[a], 8)

	state[c] += state[d]
	state[b] = bits.RotateLeft32(state[b]^state[c], 7)
}

func chacha20Block(keyWords [8]uint32, blockCounter, stream uint64) [16]uint32 {
	var initial [16]uint32
	copy(initial[0:4], chacha20Constants[:])
	copy(initial[4:12], keyWords[:])
	initial[12] = uint32(blockCounter & 0xffffffff)
	initial[13] = uint32(blockCounter >> 32)
	initial[14] = uint32(stream & 0xffffffff)
	initial[15] = uint32(stream >> 32)

	working := initial
	for round := 0; round < 10; round++ {
		quarterRound(&working, 0, 4, 8, 12)
		quarterRound(&working, 1, 5, 9, 13)
		quarterRound(&working, 2, 6, 10, 14)
		quarterRound(&working, 3, 7, 11, 15)

		quarterRound(&working, 0, 5, 10, 15)
		quarterRound(&working, 1, 6, 11, 12)
		quarterRound(&working, 2, 7, 8, 13)
		quarterRound(&working, 3, 4, 9, 14)
	}

	for i := 0; i < 16; i++ {
		working[i] += initial[i]
	}
	return working
}
