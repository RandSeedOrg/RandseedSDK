using System.Text.Json;
using System.Text.Json.Serialization;

namespace Randseed.GameServerSdk;

public sealed class GameManifest
{
    [JsonPropertyName("title")]
    [JsonRequired]
    public string Title { get; init; } = string.Empty;

    [JsonPropertyName("version")]
    [JsonRequired]
    public string Version { get; init; } = string.Empty;

    [JsonPropertyName("minPlayers")]
    [JsonRequired]
    public int MinPlayers { get; init; }

    [JsonPropertyName("maxPlayers")]
    [JsonRequired]
    public int MaxPlayers { get; init; }

    [JsonPropertyName("tickRateHz")]
    [JsonRequired]
    public double TickRateHz { get; init; }

    [JsonPropertyName("persistentTable")]
    [JsonRequired]
    public bool PersistentTable { get; init; }

    [JsonPropertyName("optionsSchema")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public JsonElement OptionsSchema { get; init; }

    [JsonPropertyName("resultDefinition")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public GameResultDefinition? ResultDefinition { get; init; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalProperties { get; init; }
}

public sealed class GameResultDefinition
{
    [JsonPropertyName("version")]
    [JsonRequired]
    public string Version { get; init; } = string.Empty;

    [JsonPropertyName("unitTypes")]
    [JsonRequired]
    public IReadOnlyList<string> UnitTypes { get; init; } = Array.Empty<string>();

    [JsonPropertyName("metrics")]
    [JsonRequired]
    public IReadOnlyList<GameMetricDefinition> Metrics { get; init; } = Array.Empty<GameMetricDefinition>();

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalProperties { get; init; }
}

public sealed class GameMetricDefinition
{
    [JsonPropertyName("id")]
    [JsonRequired]
    public string Id { get; init; } = string.Empty;

    [JsonPropertyName("scope")]
    [JsonRequired]
    public string Scope { get; init; } = string.Empty;

    [JsonPropertyName("unit")]
    [JsonRequired]
    public string Unit { get; init; } = string.Empty;

    [JsonPropertyName("unitTypes")]
    [JsonRequired]
    public IReadOnlyList<string> UnitTypes { get; init; } = Array.Empty<string>();

    [JsonPropertyName("aggregation")]
    [JsonRequired]
    public JsonElement Aggregation { get; init; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalProperties { get; init; }
}

public sealed class GameResultParticipant
{
    [JsonPropertyName("playerId")]
    [JsonRequired]
    public string PlayerId { get; init; } = string.Empty;

    [JsonPropertyName("teamId")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? TeamId { get; init; }

    [JsonPropertyName("outcome")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Outcome { get; init; }

    [JsonPropertyName("rank")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    [JsonConverter(typeof(NullableSafeIntegerInt64JsonConverter))]
    public long? Rank { get; init; }

    [JsonPropertyName("metrics")]
    [JsonRequired]
    public IReadOnlyDictionary<string, double> Metrics { get; init; } = new Dictionary<string, double>();

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalProperties { get; init; }
}

public sealed class GameResultTeam
{
    [JsonPropertyName("teamId")]
    [JsonRequired]
    public string TeamId { get; init; } = string.Empty;

    [JsonPropertyName("outcome")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Outcome { get; init; }

    [JsonPropertyName("rank")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    [JsonConverter(typeof(NullableSafeIntegerInt64JsonConverter))]
    public long? Rank { get; init; }

    [JsonPropertyName("metrics")]
    [JsonRequired]
    public IReadOnlyDictionary<string, double> Metrics { get; init; } = new Dictionary<string, double>();

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalProperties { get; init; }
}

public sealed class GameResultCorrection
{
    [JsonPropertyName("action")]
    [JsonRequired]
    public string Action { get; init; } = string.Empty;

    [JsonPropertyName("supersedesEventId")]
    [JsonRequired]
    public string SupersedesEventId { get; init; } = string.Empty;

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalProperties { get; init; }
}

public sealed class GameResultEvent
{
    [JsonPropertyName("schemaVersion")]
    [JsonRequired]
    public int SchemaVersion { get; init; }

    [JsonPropertyName("eventId")]
    [JsonRequired]
    public string EventId { get; init; } = string.Empty;

    [JsonPropertyName("matchId")]
    [JsonRequired]
    public string MatchId { get; init; } = string.Empty;

    [JsonPropertyName("unitId")]
    [JsonRequired]
    public string UnitId { get; init; } = string.Empty;

    [JsonPropertyName("unitType")]
    [JsonRequired]
    public string UnitType { get; init; } = string.Empty;

    [JsonPropertyName("rulesVersion")]
    [JsonRequired]
    public string RulesVersion { get; init; } = string.Empty;

    [JsonPropertyName("definitionVersion")]
    [JsonRequired]
    public string DefinitionVersion { get; init; } = string.Empty;

    [JsonPropertyName("occurredAt")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    [JsonConverter(typeof(NullableSafeIntegerInt64JsonConverter))]
    public long? OccurredAt { get; init; }

    [JsonPropertyName("participants")]
    [JsonRequired]
    public IReadOnlyList<GameResultParticipant> Participants { get; init; } = Array.Empty<GameResultParticipant>();

    [JsonPropertyName("teams")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<GameResultTeam>? Teams { get; init; }

    [JsonPropertyName("correction")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public GameResultCorrection? Correction { get; init; }

    [JsonExtensionData]
    public Dictionary<string, JsonElement>? AdditionalProperties { get; init; }
}

public sealed class MatchConfig
{
    [JsonPropertyName("matchId")]
    [JsonRequired]
    public string MatchId { get; init; } = string.Empty;

    [JsonPropertyName("gameId")]
    [JsonRequired]
    public string GameId { get; init; } = string.Empty;

    [JsonPropertyName("players")]
    [JsonRequired]
    public IReadOnlyList<string> Players { get; init; } = Array.Empty<string>();

    [JsonPropertyName("matchSeq")]
    [JsonRequired]
    [JsonConverter(typeof(SafeIntegerInt64JsonConverter))]
    public long MatchSeq { get; init; }

    [JsonPropertyName("options")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public JsonElement Options { get; init; }

    [JsonPropertyName("carryover")]
    [JsonRequired]
    public JsonElement Carryover { get; init; }

    [JsonPropertyName("tickRateHz")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    [JsonConverter(typeof(OptionalDoubleJsonConverter))]
    public double? TickRateHz { get; init; }
}

public sealed class InitResult
{
    [JsonPropertyName("state")]
    [JsonRequired]
    public JsonElement State { get; init; }

    [JsonPropertyName("broadcasts")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public JsonElement Broadcasts { get; init; }

    [JsonPropertyName("resultUnits")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<GameResultEvent>? ResultUnits { get; init; }
}

public sealed record TickRequest
{
    [JsonPropertyName("ctx")]
    [JsonRequired]
    public TickContext Ctx { get; init; } = null!;

    [JsonPropertyName("state")]
    [JsonRequired]
    public JsonElement State { get; init; }
}

public sealed record TickContext
{
    [JsonPropertyName("tick")]
    [JsonRequired]
    [JsonConverter(typeof(SafeIntegerInt64JsonConverter))]
    public long Tick { get; init; }

    [JsonPropertyName("inputs")]
    [JsonRequired]
    public IReadOnlyList<PlayerInput>? Inputs { get; init; }

    [JsonPropertyName("commands")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<HostCommand>? Commands { get; init; }

    [JsonPropertyName("elapsedSeconds")]
    [JsonRequired]
    public double ElapsedSeconds { get; init; }
}

public sealed class PlayerInput
{
    [JsonPropertyName("playerId")]
    [JsonRequired]
    public string PlayerId { get; init; } = string.Empty;

    [JsonPropertyName("data")]
    [JsonRequired]
    public JsonElement Data { get; init; }

    [JsonPropertyName("timestamp")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    [JsonConverter(typeof(NullableSafeIntegerInt64JsonConverter))]
    public long? Timestamp { get; init; }
}

public sealed class TickResult
{
    [JsonPropertyName("state")]
    [JsonRequired]
    public JsonElement State { get; init; }

    [JsonPropertyName("broadcasts")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public JsonElement Broadcasts { get; init; }

    [JsonPropertyName("persist")]
    [JsonRequired]
    public bool Persist { get; init; }

    [JsonPropertyName("resultUnits")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<GameResultEvent>? ResultUnits { get; init; }
}

public sealed class PlayerEventRequest
{
    [JsonPropertyName("playerId")]
    [JsonRequired]
    public string PlayerId { get; init; } = string.Empty;

    [JsonPropertyName("state")]
    [JsonRequired]
    public JsonElement State { get; init; }
}

public sealed class PlayerEventResult
{
    [JsonPropertyName("state")]
    [JsonRequired]
    public JsonElement State { get; init; }

    [JsonPropertyName("broadcasts")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public JsonElement Broadcasts { get; init; }

    [JsonPropertyName("resultUnits")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<GameResultEvent>? ResultUnits { get; init; }
}

public sealed class IsGameOverRequest
{
    [JsonPropertyName("state")]
    [JsonRequired]
    public JsonElement State { get; init; }
}

public sealed class GameResult
{
    [JsonPropertyName("over")]
    [JsonRequired]
    public bool Over { get; init; }

    [JsonPropertyName("summary")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public JsonElement Summary { get; init; }

    [JsonPropertyName("nextMatchPolicy")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public NextMatchPolicy? NextMatchPolicy { get; init; }

    [JsonPropertyName("resultUnits")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<GameResultEvent>? ResultUnits { get; init; }
}

public sealed class NextMatchPolicy
{
    [JsonPropertyName("mode")]
    [JsonRequired]
    public string Mode { get; init; } = string.Empty;

    [JsonPropertyName("timeoutMs")]
    [JsonRequired]
    public long TimeoutMs { get; init; }

    [JsonPropertyName("carryover")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public JsonElement Carryover { get; init; }
}

public sealed class CommandRequest
{
    [JsonPropertyName("command")]
    [JsonRequired]
    public HostCommand Command { get; init; } = null!;

    [JsonPropertyName("state")]
    [JsonRequired]
    public JsonElement State { get; init; }
}

public sealed class HostCommand
{
    [JsonPropertyName("type")]
    [JsonRequired]
    public string Type { get; init; } = string.Empty;

    [JsonPropertyName("data")]
    [JsonRequired]
    public JsonElement Data { get; init; }

    [JsonPropertyName("issuer")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Issuer { get; init; }
}

public sealed class CommandResult
{
    [JsonPropertyName("state")]
    [JsonRequired]
    public JsonElement State { get; init; }

    [JsonPropertyName("broadcasts")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)]
    public JsonElement Broadcasts { get; init; }

    [JsonPropertyName("resultUnits")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<GameResultEvent>? ResultUnits { get; init; }
}

public sealed class HealthResponse
{
    public HealthResponse(string status, string gameId, double uptime)
    {
        Status = status;
        GameId = gameId;
        Uptime = uptime;
    }

    [JsonPropertyName("status")]
    public string Status { get; }

    [JsonPropertyName("gameId")]
    public string GameId { get; }

    [JsonPropertyName("uptime")]
    public double Uptime { get; }
}

public sealed class ErrorResponse
{
    public ErrorResponse(string error) => Error = error;

    [JsonPropertyName("error")]
    public string Error { get; }
}
