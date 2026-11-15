using System.Buffers.Binary;
using System.Numerics;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Serialization;

namespace Randseed.GameServerSdk;

public sealed record ChaCha20State(
    [property: JsonPropertyName("seedHex")] string SeedHex,
    [property: JsonPropertyName("stream")] ulong Stream,
    [property: JsonPropertyName("wordPos")] ulong WordPos);

public sealed class ChaCha20Rng
{
    public const int RandomSeedBytes = 32;
    public const int RandomSeedHexLength = 64;
    public const string DeriveSeedDomainPrefix = "randseed-chacha20-derive-v1\0";

    private const ulong MaxSafeInteger = 9_007_199_254_740_991UL;
    private const ulong TwoPow32 = 1UL << 32;

    private static readonly uint[] Constants =
    [
        0x61707865u,
        0x3320646eu,
        0x79622d32u,
        0x6b206574u
    ];

    private readonly byte[] _seed;
    private readonly uint[] _keyWords = new uint[8];
    private ulong _stream;
    private ulong _wordPos;
    private bool _hasCachedBlock;
    private ulong _cachedBlockIndex;
    private readonly uint[] _cachedBlock = new uint[16];

    public ChaCha20Rng(ReadOnlySpan<byte> seed, ulong stream = 0, ulong wordPos = 0)
    {
        if (seed.Length != RandomSeedBytes)
        {
            throw new ArgumentException("Seed must be exactly 32 bytes.", nameof(seed));
        }

        _seed = seed.ToArray();
        for (var i = 0; i < 8; i++)
        {
            _keyWords[i] = BinaryPrimitives.ReadUInt32LittleEndian(_seed.AsSpan(i * 4, 4));
        }

        _stream = stream;
        _wordPos = wordPos;
    }

    public static ChaCha20Rng FromHex(string seedHex, ulong stream = 0, ulong wordPos = 0) =>
        new(ParseSeedHex(seedHex), stream, wordPos);

    public static ChaCha20Rng FromDerived(ReadOnlySpan<byte> rootSeed, params string[] domains) =>
        new(DeriveSeed(rootSeed, domains));

    public static ChaCha20Rng FromState(ChaCha20State state)
    {
        ArgumentNullException.ThrowIfNull(state);
        if (state.Stream > MaxSafeInteger || state.WordPos > MaxSafeInteger)
        {
            throw new ArgumentOutOfRangeException(nameof(state), "ChaCha20State stream and wordPos must be safe integers.");
        }

        return FromHex(state.SeedHex, state.Stream, state.WordPos);
    }

    public ChaCha20State GetState()
    {
        if (_stream > MaxSafeInteger || _wordPos > MaxSafeInteger)
        {
            throw new InvalidOperationException("ChaCha20Rng cursor exceeds JSON safe integer bounds.");
        }

        return new ChaCha20State(EncodeSeedHex(_seed), _stream, _wordPos);
    }

    public ulong Stream
    {
        get => _stream;
        set
        {
            if (_stream != value)
            {
                _stream = value;
                _hasCachedBlock = false;
            }
        }
    }

    public ulong WordPos
    {
        get => _wordPos;
        set => _wordPos = value;
    }

    public string SeedHex => EncodeSeedHex(_seed);

    public uint NextUInt32()
    {
        var blockIndex = _wordPos >> 4;
        var wordOffset = (int)(_wordPos & 15UL);
        if (!_hasCachedBlock || _cachedBlockIndex != blockIndex)
        {
            ComputeBlock(_keyWords, blockIndex, _stream, _cachedBlock);
            _cachedBlockIndex = blockIndex;
            _hasCachedBlock = true;
        }

        _wordPos = checked(_wordPos + 1UL);
        return _cachedBlock[wordOffset];
    }

    public ulong NextUInt64()
    {
        var lo = (ulong)NextUInt32();
        var hi = (ulong)NextUInt32();
        return lo | (hi << 32);
    }

    public double NextFloat() => NextUInt32() / 4_294_967_296d;

    public uint NextBounded(uint bound)
    {
        if (bound == 0u)
        {
            throw new ArgumentOutOfRangeException(nameof(bound), "bound must be greater than zero.");
        }

        var bound64 = (ulong)bound;
        var limit = TwoPow32 - (TwoPow32 % bound64);
        while (true)
        {
            var candidate = (ulong)NextUInt32();
            if (candidate < limit)
            {
                return (uint)(candidate % bound64);
            }
        }
    }

    public int NextIntRange(int minInclusive, int maxInclusive)
    {
        if (minInclusive > maxInclusive)
        {
            throw new ArgumentOutOfRangeException(nameof(minInclusive), "minInclusive must be <= maxInclusive.");
        }

        var span = (ulong)((long)maxInclusive - minInclusive);
        if (span == uint.MaxValue)
        {
            return (int)((long)minInclusive + NextUInt32());
        }

        var offset = NextBounded((uint)(span + 1UL));
        return (int)((long)minInclusive + offset);
    }

    public bool NextBool() => NextBounded(2u) == 1u;

    public void Shuffle<T>(IList<T> items)
    {
        ArgumentNullException.ThrowIfNull(items);
        for (var i = items.Count - 1; i >= 1; i--)
        {
            var j = (int)NextBounded((uint)(i + 1));
            (items[i], items[j]) = (items[j], items[i]);
        }
    }

    public static bool IsValidSeedHex(string? value)
    {
        if (value is null || value.Length != RandomSeedHexLength)
        {
            return false;
        }

        foreach (var ch in value)
        {
            if (!((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f')))
            {
                return false;
            }
        }

        return true;
    }

    public static byte[] ParseSeedHex(string seedHex)
    {
        if (!IsValidSeedHex(seedHex))
        {
            throw new ArgumentException("randomSeed must be a 64-character lowercase hex string.", nameof(seedHex));
        }

        return Convert.FromHexString(seedHex);
    }

    public static string EncodeSeedHex(ReadOnlySpan<byte> seed)
    {
        if (seed.Length != RandomSeedBytes)
        {
            throw new ArgumentException("Seed must be exactly 32 bytes.", nameof(seed));
        }

        return Convert.ToHexString(seed).ToLowerInvariant();
    }

    public static uint LegacySeedFromBytes(ReadOnlySpan<byte> seed)
    {
        if (seed.Length != RandomSeedBytes)
        {
            throw new ArgumentException("Seed must be exactly 32 bytes.", nameof(seed));
        }

        return BinaryPrimitives.ReadUInt32LittleEndian(seed[..4]);
    }

    public static byte[] DeriveSeed(ReadOnlySpan<byte> rootSeed, params byte[][] domains)
    {
        if (rootSeed.Length != RandomSeedBytes)
        {
            throw new ArgumentException("rootSeed must be exactly 32 bytes.", nameof(rootSeed));
        }

        ArgumentNullException.ThrowIfNull(domains);
        using var hasher = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        hasher.AppendData(Encoding.UTF8.GetBytes(DeriveSeedDomainPrefix));
        hasher.AppendData(rootSeed);

        Span<byte> lenBytes = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(lenBytes, checked((uint)domains.Length));
        hasher.AppendData(lenBytes);

        foreach (var domain in domains)
        {
            ArgumentNullException.ThrowIfNull(domain);
            BinaryPrimitives.WriteUInt32LittleEndian(lenBytes, checked((uint)domain.Length));
            hasher.AppendData(lenBytes);
            hasher.AppendData(domain);
        }

        return hasher.GetHashAndReset();
    }

    public static byte[] DeriveSeed(ReadOnlySpan<byte> rootSeed, params string[] domains)
    {
        ArgumentNullException.ThrowIfNull(domains);
        var byteDomains = new byte[domains.Length][];
        for (var i = 0; i < domains.Length; i++)
        {
            ArgumentNullException.ThrowIfNull(domains[i]);
            byteDomains[i] = Encoding.UTF8.GetBytes(domains[i]);
        }

        return DeriveSeed(rootSeed, byteDomains);
    }

    public static string DeriveSeedHex(string rootSeedHex, params string[] domains) =>
        EncodeSeedHex(DeriveSeed(ParseSeedHex(rootSeedHex), domains));

    private static void QuarterRound(Span<uint> state, int a, int b, int c, int d)
    {
        state[a] = unchecked(state[a] + state[b]);
        state[d] = BitOperations.RotateLeft(state[d] ^ state[a], 16);

        state[c] = unchecked(state[c] + state[d]);
        state[b] = BitOperations.RotateLeft(state[b] ^ state[c], 12);

        state[a] = unchecked(state[a] + state[b]);
        state[d] = BitOperations.RotateLeft(state[d] ^ state[a], 8);

        state[c] = unchecked(state[c] + state[d]);
        state[b] = BitOperations.RotateLeft(state[b] ^ state[c], 7);
    }

    private static void ComputeBlock(ReadOnlySpan<uint> keyWords, ulong blockCounter, ulong stream, Span<uint> output)
    {
        Span<uint> initial = stackalloc uint[16];
        Constants.AsSpan().CopyTo(initial[..4]);
        keyWords.CopyTo(initial.Slice(4, 8));
        initial[12] = (uint)(blockCounter & 0xffff_ffffUL);
        initial[13] = (uint)(blockCounter >> 32);
        initial[14] = (uint)(stream & 0xffff_ffffUL);
        initial[15] = (uint)(stream >> 32);

        initial.CopyTo(output);
        for (var round = 0; round < 10; round++)
        {
            QuarterRound(output, 0, 4, 8, 12);
            QuarterRound(output, 1, 5, 9, 13);
            QuarterRound(output, 2, 6, 10, 14);
            QuarterRound(output, 3, 7, 11, 15);

            QuarterRound(output, 0, 5, 10, 15);
            QuarterRound(output, 1, 6, 11, 12);
            QuarterRound(output, 2, 7, 8, 13);
            QuarterRound(output, 3, 4, 9, 14);
        }

        for (var i = 0; i < 16; i++)
        {
            output[i] = unchecked(output[i] + initial[i]);
        }
    }
}
