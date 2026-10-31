#pragma once

#include <cmath>
#include <cstdint>
#include <limits>

namespace randseed {

class Mulberry32 final {
public:
    explicit Mulberry32(double seed) noexcept
        : state_(static_cast<std::uint32_t>(to_int32(seed))) {}

    [[nodiscard]] static std::int32_t to_int32(double value) noexcept {
        if (!std::isfinite(value) || value == 0.0) {
            return 0;
        }

        constexpr double modulus = 4294967296.0;
        double remainder = std::fmod(std::trunc(value), modulus);
        if (remainder < 0.0) {
            remainder += modulus;
        }

        const auto unsigned_value = static_cast<std::uint32_t>(remainder);
        if (unsigned_value <=
            static_cast<std::uint32_t>(std::numeric_limits<std::int32_t>::max())) {
            return static_cast<std::int32_t>(unsigned_value);
        }

        const auto signed_value =
            static_cast<std::int64_t>(unsigned_value) - 4294967296LL;
        return static_cast<std::int32_t>(signed_value);
    }

    [[nodiscard]] std::uint32_t next_u32() noexcept {
        state_ += 0x6D2B79F5U;
        std::uint32_t value = state_;
        value = (value ^ (value >> 15U)) * (value | 1U);
        value ^= value + ((value ^ (value >> 7U)) * (value | 61U));
        return value ^ (value >> 14U);
    }

    [[nodiscard]] double next_float() noexcept {
        return static_cast<double>(next_u32()) / 4294967296.0;
    }

private:
    std::uint32_t state_;
};

} // namespace randseed
