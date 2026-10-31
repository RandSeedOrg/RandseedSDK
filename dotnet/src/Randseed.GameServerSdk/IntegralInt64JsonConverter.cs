using System.Text.Json;
using System.Text.Json.Serialization;

namespace Randseed.GameServerSdk;

public sealed class SafeIntegerInt64JsonConverter : JsonConverter<long>
{
    private const double MaxSafeInteger = 9_007_199_254_740_991d;

    public override long Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType != JsonTokenType.Number ||
            !reader.TryGetDouble(out var number) ||
            !double.IsFinite(number) ||
            Math.Truncate(number) != number ||
            number < -MaxSafeInteger ||
            number > MaxSafeInteger)
        {
            throw new JsonException("Expected a safe integer JSON number.");
        }

        return (long)number;
    }

    public override void Write(Utf8JsonWriter writer, long value, JsonSerializerOptions options) =>
        writer.WriteNumberValue(value);
}

public sealed class NullableSafeIntegerInt64JsonConverter : JsonConverter<long?>
{
    public override bool HandleNull => true;

    public override long? Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType == JsonTokenType.Null)
        {
            return null;
        }

        return new SafeIntegerInt64JsonConverter().Read(ref reader, typeof(long), options);
    }

    public override void Write(Utf8JsonWriter writer, long? value, JsonSerializerOptions options)
    {
        if (value is long number)
        {
            writer.WriteNumberValue(number);
        }
        else
        {
            writer.WriteNullValue();
        }
    }
}
