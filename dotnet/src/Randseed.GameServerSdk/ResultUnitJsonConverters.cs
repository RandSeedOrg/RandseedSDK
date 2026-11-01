using System.Text.Json;
using System.Text.Json.Serialization;

namespace Randseed.GameServerSdk;

internal sealed class ResultUnitOptionalNullConverterFactory : JsonConverterFactory
{
    public override bool CanConvert(Type typeToConvert) =>
        typeToConvert == typeof(GameResultEvent) ||
        typeToConvert == typeof(GameResultParticipant) ||
        typeToConvert == typeof(GameResultTeam);

    public override JsonConverter CreateConverter(Type typeToConvert, JsonSerializerOptions options)
    {
        if (typeToConvert == typeof(GameResultEvent))
        {
            return new ResultUnitOptionalNullConverter<GameResultEvent>(this);
        }
        if (typeToConvert == typeof(GameResultParticipant))
        {
            return new ResultUnitOptionalNullConverter<GameResultParticipant>(this);
        }
        if (typeToConvert == typeof(GameResultTeam))
        {
            return new ResultUnitOptionalNullConverter<GameResultTeam>(this);
        }
        throw new InvalidOperationException($"Unsupported Result Unit type {typeToConvert.Name}.");
    }

    internal JsonSerializerOptions WithoutThisConverter(JsonSerializerOptions options)
    {
        var clone = new JsonSerializerOptions(options);
        clone.Converters.Remove(this);
        return clone;
    }

    internal static void RejectExplicitNulls(JsonElement value, Type type)
    {
        if (value.ValueKind != JsonValueKind.Object)
        {
            return;
        }
        var optionalFields = type == typeof(GameResultEvent)
            ? new[] { "occurredAt", "teams", "correction" }
            : type == typeof(GameResultParticipant)
                ? new[] { "teamId", "outcome", "rank" }
                : new[] { "outcome", "rank" };
        foreach (var field in optionalFields)
        {
            if (value.TryGetProperty(field, out var property) && property.ValueKind == JsonValueKind.Null)
            {
                throw new JsonException($"Optional field {field} must be omitted rather than null.");
            }
        }
        if (type == typeof(GameResultEvent))
        {
            if (value.TryGetProperty("participants", out var participants) &&
                participants.ValueKind == JsonValueKind.Array)
            {
                foreach (var participant in participants.EnumerateArray())
                {
                    RejectExplicitNulls(participant, typeof(GameResultParticipant));
                }
            }
            if (value.TryGetProperty("teams", out var teams) &&
                teams.ValueKind == JsonValueKind.Array)
            {
                foreach (var team in teams.EnumerateArray())
                {
                    RejectExplicitNulls(team, typeof(GameResultTeam));
                }
            }
        }
    }
}

internal sealed class ResultUnitOptionalNullConverter<T>(
    ResultUnitOptionalNullConverterFactory factory) : JsonConverter<T>
    where T : class
{
    public override T? Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        using var document = JsonDocument.ParseValue(ref reader);
        ResultUnitOptionalNullConverterFactory.RejectExplicitNulls(document.RootElement, typeof(T));
        return JsonSerializer.Deserialize<T>(
            document.RootElement.GetRawText(),
            factory.WithoutThisConverter(options))
            ?? throw new JsonException($"Could not deserialize {typeof(T).Name}.");
    }

    public override void Write(Utf8JsonWriter writer, T value, JsonSerializerOptions options) =>
        JsonSerializer.Serialize(writer, value, factory.WithoutThisConverter(options));
}
