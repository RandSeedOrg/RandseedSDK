#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace randseed {

inline constexpr std::size_t random_seed_bytes = 32U;
inline constexpr std::size_t random_seed_hex_len = 64U;
inline constexpr std::string_view derive_seed_domain_prefix{
    "randseed-chacha20-derive-v1\0", 28U};

using SeedBytes = std::array<std::uint8_t, random_seed_bytes>;

struct ChaCha20State {
    std::string seed_hex;
    std::uint64_t stream{0};
    std::uint64_t word_pos{0};
};

[[nodiscard]] inline bool is_valid_seed_hex(std::string_view value) noexcept {
    if (value.size() != random_seed_hex_len) {
        return false;
    }
    for (const char ch : value) {
        const auto u = static_cast<unsigned char>(ch);
        if (!((u >= '0' && u <= '9') || (u >= 'a' && u <= 'f'))) {
            return false;
        }
    }
    return true;
}

namespace detail {

[[nodiscard]] constexpr std::uint32_t rotl32(
    std::uint32_t value,
    unsigned int shift) noexcept {
    return (value << shift) | (value >> (32U - shift));
}

[[nodiscard]] constexpr std::uint32_t rotr32(
    std::uint32_t value,
    unsigned int shift) noexcept {
    return (value >> shift) | (value << (32U - shift));
}

[[nodiscard]] inline int hex_nibble(unsigned char ch) noexcept {
    if (ch >= '0' && ch <= '9') {
        return static_cast<int>(ch - '0');
    }
    if (ch >= 'a' && ch <= 'f') {
        return static_cast<int>(ch - 'a' + 10);
    }
    return -1;
}

inline void quarter_round(
    std::array<std::uint32_t, 16>& state,
    std::size_t a,
    std::size_t b,
    std::size_t c,
    std::size_t d) noexcept {
    state[a] += state[b];
    state[d] = rotl32(state[d] ^ state[a], 16U);

    state[c] += state[d];
    state[b] = rotl32(state[b] ^ state[c], 12U);

    state[a] += state[b];
    state[d] = rotl32(state[d] ^ state[a], 8U);

    state[c] += state[d];
    state[b] = rotl32(state[b] ^ state[c], 7U);
}

[[nodiscard]] inline std::array<std::uint32_t, 16> chacha20_block(
    const std::array<std::uint32_t, 8>& key_words,
    std::uint64_t block_counter,
    std::uint64_t stream) noexcept {
    std::array<std::uint32_t, 16> initial{
        0x61707865U,
        0x3320646eU,
        0x79622d32U,
        0x6b206574U,
        key_words[0],
        key_words[1],
        key_words[2],
        key_words[3],
        key_words[4],
        key_words[5],
        key_words[6],
        key_words[7],
        static_cast<std::uint32_t>(block_counter & 0xffffffffULL),
        static_cast<std::uint32_t>(block_counter >> 32U),
        static_cast<std::uint32_t>(stream & 0xffffffffULL),
        static_cast<std::uint32_t>(stream >> 32U)};

    auto working = initial;
    for (int round = 0; round < 10; ++round) {
        quarter_round(working, 0, 4, 8, 12);
        quarter_round(working, 1, 5, 9, 13);
        quarter_round(working, 2, 6, 10, 14);
        quarter_round(working, 3, 7, 11, 15);

        quarter_round(working, 0, 5, 10, 15);
        quarter_round(working, 1, 6, 11, 12);
        quarter_round(working, 2, 7, 8, 13);
        quarter_round(working, 3, 4, 9, 14);
    }

    for (std::size_t i = 0; i < 16; ++i) {
        working[i] += initial[i];
    }
    return working;
}

class Sha256 final {
public:
    void update(const std::uint8_t* data, std::size_t len) noexcept {
        total_bytes_ += static_cast<std::uint64_t>(len);
        if (buffer_len_ > 0) {
            const std::size_t take =
                len < (64U - buffer_len_) ? len : (64U - buffer_len_);
            for (std::size_t i = 0; i < take; ++i) {
                buffer_[buffer_len_ + i] = data[i];
            }
            buffer_len_ += take;
            data += take;
            len -= take;
            if (buffer_len_ == 64U) {
                compress(buffer_.data());
                buffer_len_ = 0;
            }
        }
        while (len >= 64U) {
            compress(data);
            data += 64U;
            len -= 64U;
        }
        if (len > 0U) {
            for (std::size_t i = 0; i < len; ++i) {
                buffer_[i] = data[i];
            }
            buffer_len_ = len;
        }
    }

    void update_u32_le(std::uint32_t value) noexcept {
        const std::array<std::uint8_t, 4> bytes{
            static_cast<std::uint8_t>(value & 0xffU),
            static_cast<std::uint8_t>((value >> 8U) & 0xffU),
            static_cast<std::uint8_t>((value >> 16U) & 0xffU),
            static_cast<std::uint8_t>((value >> 24U) & 0xffU)};
        update(bytes.data(), bytes.size());
    }

    [[nodiscard]] SeedBytes finalize() noexcept {
        const std::uint64_t total_bits = total_bytes_ * 8ULL;
        buffer_[buffer_len_++] = 0x80U;
        if (buffer_len_ > 56U) {
            while (buffer_len_ < 64U) {
                buffer_[buffer_len_++] = 0U;
            }
            compress(buffer_.data());
            buffer_len_ = 0;
        }
        while (buffer_len_ < 56U) {
            buffer_[buffer_len_++] = 0U;
        }
        for (std::size_t i = 0; i < 8U; ++i) {
            buffer_[56U + i] = static_cast<std::uint8_t>(
                (total_bits >> ((7U - i) * 8U)) & 0xffULL);
        }
        compress(buffer_.data());

        SeedBytes out{};
        for (std::size_t i = 0; i < 8U; ++i) {
            out[i * 4U] = static_cast<std::uint8_t>((state_[i] >> 24U) & 0xffU);
            out[i * 4U + 1U] =
                static_cast<std::uint8_t>((state_[i] >> 16U) & 0xffU);
            out[i * 4U + 2U] =
                static_cast<std::uint8_t>((state_[i] >> 8U) & 0xffU);
            out[i * 4U + 3U] = static_cast<std::uint8_t>(state_[i] & 0xffU);
        }
        return out;
    }

private:
    static constexpr std::array<std::uint32_t, 64> k_table{
        0x428a2f98U, 0x71374491U, 0xb5c0fbcfU, 0xe9b5dba5U, 0x3956c25bU,
        0x59f111f1U, 0x923f82a4U, 0xab1c5ed5U, 0xd807aa98U, 0x12835b01U,
        0x243185beU, 0x550c7dc3U, 0x72be5d74U, 0x80deb1feU, 0x9bdc06a7U,
        0xc19bf174U, 0xe49b69c1U, 0xefbe4786U, 0x0fc19dc6U, 0x240ca1ccU,
        0x2de92c6fU, 0x4a7484aaU, 0x5cb0a9dcU, 0x76f988daU, 0x983e5152U,
        0xa831c66dU, 0xb00327c8U, 0xbf597fc7U, 0xc6e00bf3U, 0xd5a79147U,
        0x06ca6351U, 0x14292967U, 0x27b70a85U, 0x2e1b2138U, 0x4d2c6dfcU,
        0x53380d13U, 0x650a7354U, 0x766a0abbU, 0x81c2c92eU, 0x92722c85U,
        0xa2bfe8a1U, 0xa81a664bU, 0xc24b8b70U, 0xc76c51a3U, 0xd192e819U,
        0xd6990624U, 0xf40e3585U, 0x106aa070U, 0x19a4c116U, 0x1e376c08U,
        0x2748774cU, 0x34b0bcb5U, 0x391c0cb3U, 0x4ed8aa4aU, 0x5b9cca4fU,
        0x682e6ff3U, 0x748f82eeU, 0x78a5636fU, 0x84c87814U, 0x8cc70208U,
        0x90befffaU, 0xa4506cebU, 0xbef9a3f7U, 0xc67178f2U};

    void compress(const std::uint8_t* block) noexcept {
        std::array<std::uint32_t, 64> w{};
        for (std::size_t i = 0; i < 16U; ++i) {
            const std::size_t off = i * 4U;
            w[i] = (static_cast<std::uint32_t>(block[off]) << 24U) |
                (static_cast<std::uint32_t>(block[off + 1U]) << 16U) |
                (static_cast<std::uint32_t>(block[off + 2U]) << 8U) |
                static_cast<std::uint32_t>(block[off + 3U]);
        }
        for (std::size_t i = 16U; i < 64U; ++i) {
            const std::uint32_t s0 =
                rotr32(w[i - 15U], 7U) ^ rotr32(w[i - 15U], 18U) ^
                (w[i - 15U] >> 3U);
            const std::uint32_t s1 =
                rotr32(w[i - 2U], 17U) ^ rotr32(w[i - 2U], 19U) ^
                (w[i - 2U] >> 10U);
            w[i] = w[i - 16U] + s0 + w[i - 7U] + s1;
        }

        std::uint32_t a = state_[0];
        std::uint32_t b = state_[1];
        std::uint32_t c = state_[2];
        std::uint32_t d = state_[3];
        std::uint32_t e = state_[4];
        std::uint32_t f = state_[5];
        std::uint32_t g = state_[6];
        std::uint32_t h = state_[7];

        for (std::size_t i = 0; i < 64U; ++i) {
            const std::uint32_t s1 =
                rotr32(e, 6U) ^ rotr32(e, 11U) ^ rotr32(e, 25U);
            const std::uint32_t ch = (e & f) ^ ((~e) & g);
            const std::uint32_t temp1 = h + s1 + ch + k_table[i] + w[i];
            const std::uint32_t s0 =
                rotr32(a, 2U) ^ rotr32(a, 13U) ^ rotr32(a, 22U);
            const std::uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
            const std::uint32_t temp2 = s0 + maj;

            h = g;
            g = f;
            f = e;
            e = d + temp1;
            d = c;
            c = b;
            b = a;
            a = temp1 + temp2;
        }

        state_[0] += a;
        state_[1] += b;
        state_[2] += c;
        state_[3] += d;
        state_[4] += e;
        state_[5] += f;
        state_[6] += g;
        state_[7] += h;
    }

    std::array<std::uint32_t, 8> state_{
        0x6a09e667U,
        0xbb67ae85U,
        0x3c6ef372U,
        0xa54ff53aU,
        0x510e527fU,
        0x9b05688cU,
        0x1f83d9abU,
        0x5be0cd19U};
    std::array<std::uint8_t, 64> buffer_{};
    std::size_t buffer_len_{0};
    std::uint64_t total_bytes_{0};
};

} // namespace detail

[[nodiscard]] inline SeedBytes parse_seed_hex(std::string_view seed_hex) {
    if (seed_hex.size() != random_seed_hex_len) {
        throw std::invalid_argument(
            "randomSeed must be a 64-character lowercase hex string");
    }
    SeedBytes out{};
    for (std::size_t i = 0; i < random_seed_bytes; ++i) {
        const int hi = detail::hex_nibble(
            static_cast<unsigned char>(seed_hex[i * 2U]));
        const int lo = detail::hex_nibble(
            static_cast<unsigned char>(seed_hex[i * 2U + 1U]));
        if (hi < 0 || lo < 0) {
            throw std::invalid_argument(
                "randomSeed must be a 64-character lowercase hex string");
        }
        out[i] = static_cast<std::uint8_t>((hi << 4) | lo);
    }
    return out;
}

[[nodiscard]] inline std::string encode_seed_hex(const SeedBytes& seed) {
    static constexpr char hex_chars[] = "0123456789abcdef";
    std::string out(random_seed_hex_len, '0');
    for (std::size_t i = 0; i < random_seed_bytes; ++i) {
        out[i * 2U] = hex_chars[(seed[i] >> 4U) & 0x0fU];
        out[i * 2U + 1U] = hex_chars[seed[i] & 0x0fU];
    }
    return out;
}

[[nodiscard]] inline std::uint32_t legacy_seed_from_bytes(
    const SeedBytes& seed) noexcept {
    return static_cast<std::uint32_t>(seed[0]) |
        (static_cast<std::uint32_t>(seed[1]) << 8U) |
        (static_cast<std::uint32_t>(seed[2]) << 16U) |
        (static_cast<std::uint32_t>(seed[3]) << 24U);
}

[[nodiscard]] inline SeedBytes derive_seed(
    const SeedBytes& root_seed,
    const std::vector<std::string_view>& domains) {
    detail::Sha256 hasher;
    hasher.update(
        reinterpret_cast<const std::uint8_t*>(derive_seed_domain_prefix.data()),
        derive_seed_domain_prefix.size());
    hasher.update(root_seed.data(), root_seed.size());
    hasher.update_u32_le(static_cast<std::uint32_t>(domains.size()));
    for (const std::string_view domain : domains) {
        hasher.update_u32_le(static_cast<std::uint32_t>(domain.size()));
        if (!domain.empty()) {
            hasher.update(
                reinterpret_cast<const std::uint8_t*>(domain.data()),
                domain.size());
        }
    }
    return hasher.finalize();
}

[[nodiscard]] inline std::string derive_seed_hex(
    std::string_view root_seed_hex,
    const std::vector<std::string_view>& domains) {
    return encode_seed_hex(derive_seed(parse_seed_hex(root_seed_hex), domains));
}

class ChaCha20Rng final {
public:
    explicit ChaCha20Rng(
        const SeedBytes& seed,
        std::uint64_t stream = 0,
        std::uint64_t word_pos = 0) noexcept
        : seed_(seed), stream_(stream), word_pos_(word_pos) {
        for (std::size_t i = 0; i < 8U; ++i) {
            const std::size_t off = i * 4U;
            key_words_[i] = static_cast<std::uint32_t>(seed_[off]) |
                (static_cast<std::uint32_t>(seed_[off + 1U]) << 8U) |
                (static_cast<std::uint32_t>(seed_[off + 2U]) << 16U) |
                (static_cast<std::uint32_t>(seed_[off + 3U]) << 24U);
        }
    }

    [[nodiscard]] static ChaCha20Rng from_seed_hex(
        std::string_view seed_hex,
        std::uint64_t stream = 0,
        std::uint64_t word_pos = 0) {
        return ChaCha20Rng(parse_seed_hex(seed_hex), stream, word_pos);
    }

    [[nodiscard]] static ChaCha20Rng from_derived(
        const SeedBytes& root_seed,
        const std::vector<std::string_view>& domains) {
        return ChaCha20Rng(derive_seed(root_seed, domains));
    }

    [[nodiscard]] static ChaCha20Rng from_state(const ChaCha20State& state) {
        constexpr std::uint64_t max_safe_integer = 9007199254740991ULL;
        if (state.stream > max_safe_integer ||
            state.word_pos > max_safe_integer) {
            throw std::invalid_argument(
                "ChaCha20State stream and wordPos must be safe integers");
        }
        return from_seed_hex(state.seed_hex, state.stream, state.word_pos);
    }

    [[nodiscard]] ChaCha20State state() const {
        return ChaCha20State{
            .seed_hex = encode_seed_hex(seed_),
            .stream = stream_,
            .word_pos = word_pos_};
    }

    void set_stream(std::uint64_t stream) noexcept {
        if (stream_ != stream) {
            stream_ = stream;
            has_cached_block_ = false;
        }
    }

    [[nodiscard]] std::uint64_t stream() const noexcept {
        return stream_;
    }

    void set_word_pos(std::uint64_t word_pos) noexcept {
        word_pos_ = word_pos;
    }

    [[nodiscard]] std::uint64_t word_pos() const noexcept {
        return word_pos_;
    }

    [[nodiscard]] std::uint32_t next_u32() noexcept {
        const std::uint64_t block_index = word_pos_ >> 4U;
        const auto word_offset = static_cast<std::size_t>(word_pos_ & 15ULL);
        if (!has_cached_block_ || cached_block_index_ != block_index) {
            cached_block_ =
                detail::chacha20_block(key_words_, block_index, stream_);
            cached_block_index_ = block_index;
            has_cached_block_ = true;
        }
        ++word_pos_;
        return cached_block_[word_offset];
    }

    [[nodiscard]] std::uint64_t next_u64() noexcept {
        const auto lo = static_cast<std::uint64_t>(next_u32());
        const auto hi = static_cast<std::uint64_t>(next_u32());
        return lo | (hi << 32U);
    }

    [[nodiscard]] double next_float() noexcept {
        return static_cast<double>(next_u32()) / 4294967296.0;
    }

    [[nodiscard]] std::uint32_t next_bounded(std::uint32_t bound) {
        if (bound == 0U) {
            throw std::invalid_argument(
                "ChaCha20Rng::next_bounded requires bound > 0");
        }
        constexpr std::uint64_t two_pow_32 = 1ULL << 32U;
        const auto bound64 = static_cast<std::uint64_t>(bound);
        const std::uint64_t limit = two_pow_32 - (two_pow_32 % bound64);
        for (;;) {
            const auto candidate = static_cast<std::uint64_t>(next_u32());
            if (candidate < limit) {
                return static_cast<std::uint32_t>(candidate % bound64);
            }
        }
    }

    [[nodiscard]] std::int32_t next_int_range(
        std::int32_t min_val,
        std::int32_t max_inclusive) {
        if (min_val > max_inclusive) {
            throw std::invalid_argument(
                "ChaCha20Rng::next_int_range requires min_val <= max_inclusive");
        }
        const auto span = static_cast<std::uint64_t>(
            static_cast<std::int64_t>(max_inclusive) -
            static_cast<std::int64_t>(min_val));
        if (span == 0xffffffffULL) {
            return static_cast<std::int32_t>(
                static_cast<std::int64_t>(min_val) +
                static_cast<std::int64_t>(next_u32()));
        }
        const std::uint32_t offset =
            next_bounded(static_cast<std::uint32_t>(span + 1ULL));
        return static_cast<std::int32_t>(
            static_cast<std::int64_t>(min_val) +
            static_cast<std::int64_t>(offset));
    }

    [[nodiscard]] bool next_bool() {
        return next_bounded(2U) == 1U;
    }

    template <typename T>
    void shuffle(std::vector<T>& items) {
        for (std::size_t i = items.size(); i > 1U; --i) {
            const std::size_t idx = i - 1U;
            const auto j = static_cast<std::size_t>(
                next_bounded(static_cast<std::uint32_t>(idx + 1U)));
            std::swap(items[idx], items[j]);
        }
    }

private:
    SeedBytes seed_{};
    std::array<std::uint32_t, 8> key_words_{};
    std::uint64_t stream_{0};
    std::uint64_t word_pos_{0};
    bool has_cached_block_{false};
    std::uint64_t cached_block_index_{0};
    std::array<std::uint32_t, 16> cached_block_{};
};

} // namespace randseed
