namespace Randseed.GameServerSdk;

public sealed class Mulberry32
{
    private uint _state;

    public Mulberry32(double seed) => _state = unchecked((uint)ToInt32(seed));

    public uint NextUInt32()
    {
        var value = unchecked(_state += 0x6D2B79F5u);
        value = unchecked((value ^ (value >> 15)) * (value | 1u));
        value ^= unchecked(value + ((value ^ (value >> 7)) * (value | 61u)));
        return value ^ (value >> 14);
    }

    public double NextFloat() => NextUInt32() / 4_294_967_296d;

    public static int ToInt32(double value)
    {
        if (!double.IsFinite(value) || value == 0d)
        {
            return 0;
        }

        var modulo = Math.Truncate(value) % 4_294_967_296d;
        if (modulo < 0d)
        {
            modulo += 4_294_967_296d;
        }

        if (modulo >= 2_147_483_648d)
        {
            modulo -= 4_294_967_296d;
        }

        return (int)modulo;
    }
}
