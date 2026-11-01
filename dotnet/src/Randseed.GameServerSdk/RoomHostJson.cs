using System.Text.Json;

namespace Randseed.GameServerSdk;

public static class RoomHostJson
{
    private static readonly JsonSerializerOptions HostOptionsValue = CreateOptions();

    public static JsonSerializerOptions CreateOptions()
    {
        var options = new JsonSerializerOptions
        {
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
            PropertyNameCaseInsensitive = false
        };
        options.Converters.Add(new ResultUnitOptionalNullConverterFactory());
        return options;
    }

    internal static JsonSerializerOptions HostOptions => HostOptionsValue;
}

public static class GameJson
{
    private static readonly JsonSerializerOptions Options = RoomHostJson.CreateOptions();

    public static JsonElement ToElement<T>(T value) =>
        JsonSerializer.SerializeToElement(value, Options);
}
