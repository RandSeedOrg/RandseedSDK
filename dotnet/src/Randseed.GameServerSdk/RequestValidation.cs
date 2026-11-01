using System.Text;
using System.Text.Json;

namespace Randseed.GameServerSdk;

internal static class RequestValidation
{
    private const long MaxSafeInteger = 9_007_199_254_740_991L;

    public static string? Manifest(GameManifest? manifest)
    {
        if (manifest is null)
        {
            return "Game manifest is required";
        }

        if (string.IsNullOrWhiteSpace(manifest.Title) || string.IsNullOrWhiteSpace(manifest.Version))
        {
            return "Manifest title and version must be non-empty";
        }

        if (manifest.MinPlayers < 1 ||
            manifest.MaxPlayers < manifest.MinPlayers ||
            manifest.MaxPlayers > 100)
        {
            return "Manifest player limits are invalid";
        }

        if (!double.IsFinite(manifest.TickRateHz) || manifest.TickRateHz is < 1 or > 60)
        {
            return "Manifest tick rate must be between 1 and 60";
        }

        if (IsDefined(manifest.OptionsSchema) && manifest.OptionsSchema.ValueKind != JsonValueKind.Object)
        {
            return "Manifest optionsSchema must be a JSON object";
        }

        if (manifest.AdditionalProperties is { Count: > 0 })
        {
            return "Manifest contains unsupported fields";
        }

        return manifest.ResultDefinition is null ? null : ResultDefinition(manifest.ResultDefinition);
    }

    public static string? ResultDefinition(GameResultDefinition? definition)
    {
        if (definition is null || HasAdditionalProperties(definition.AdditionalProperties))
        {
            return "Result definition is invalid";
        }
        if (!ValidIdentifier(definition.Version) || !UniqueIdentifiers(definition.UnitTypes, out var unitTypes) ||
            definition.Metrics is null || definition.Metrics.Count > 128)
        {
            return "Result definition version, unit types, or metric count is invalid";
        }

        var metrics = new Dictionary<string, GameMetricDefinition>(StringComparer.Ordinal);
        foreach (var metric in definition.Metrics)
        {
            if (metric is null || HasAdditionalProperties(metric.AdditionalProperties) ||
                !ValidIdentifier(metric.Id) || !ValidIdentifier(metric.Unit) ||
                metric.Scope is not ("participant" or "team") ||
                !UniqueIdentifiers(metric.UnitTypes, out var metricUnits) ||
                metricUnits.Any(unitType => !unitTypes.Contains(unitType)) ||
                !metrics.TryAdd(metric.Id, metric))
            {
                return "Result metric declaration is invalid";
            }
            if (!TryReadAggregation(metric.Aggregation, out var aggregation, out _, out _))
            {
                return $"Result metric {metric.Id} aggregation is invalid";
            }
            if (aggregation is not ("sum" or "min" or "max" or "ratio"))
            {
                return $"Result metric {metric.Id} aggregation is unsupported";
            }
        }

        foreach (var metric in definition.Metrics)
        {
            if (!TryReadAggregation(metric.Aggregation, out var aggregation, out var numeratorId, out var denominatorId) ||
                aggregation != "ratio")
            {
                continue;
            }
            if (numeratorId == metric.Id || denominatorId == metric.Id ||
                !metrics.TryGetValue(numeratorId, out var numerator) ||
                !metrics.TryGetValue(denominatorId, out var denominator) ||
                numerator.Scope != metric.Scope || denominator.Scope != metric.Scope ||
                !IsSimpleAggregation(numerator.Aggregation, "sum") ||
                !IsSimpleAggregation(denominator.Aggregation, "sum") ||
                metric.UnitTypes.Any(unitType =>
                    !numerator.UnitTypes.Contains(unitType, StringComparer.Ordinal) ||
                    !denominator.UnitTypes.Contains(unitType, StringComparer.Ordinal)))
            {
                return $"Ratio metric {metric.Id} must reference same-scope sum metrics for the same unit types";
            }
        }
        return null;
    }

    public static string? ResultEvent(GameResultEvent? resultEvent, GameResultDefinition? definition = null)
    {
        if (resultEvent is null || HasAdditionalProperties(resultEvent.AdditionalProperties) ||
            resultEvent.SchemaVersion != 1 ||
            !ValidIdentifier(resultEvent.EventId) || !ValidIdentifier(resultEvent.MatchId) ||
            !ValidIdentifier(resultEvent.UnitId) || !ValidIdentifier(resultEvent.UnitType) ||
            !ValidIdentifier(resultEvent.RulesVersion) || !ValidIdentifier(resultEvent.DefinitionVersion) ||
            resultEvent.Participants is null || resultEvent.Participants.Count > 1000 ||
            resultEvent.Teams is { Count: > 256 } ||
            resultEvent.OccurredAt is long timestamp && (timestamp < 0 || timestamp > MaxSafeInteger))
        {
            return "Result event shape or size is invalid";
        }
        if (resultEvent.Correction is not null &&
            (HasAdditionalProperties(resultEvent.Correction.AdditionalProperties) ||
             resultEvent.Correction.Action is not ("replace" or "retract") ||
             !ValidIdentifier(resultEvent.Correction.SupersedesEventId) ||
             resultEvent.Correction.SupersedesEventId == resultEvent.EventId))
        {
            return "Result event correction is invalid";
        }
        var isRetraction = resultEvent.Correction?.Action == "retract";
        var teams = resultEvent.Teams ?? Array.Empty<GameResultTeam>();
        if (resultEvent.Participants.Count == 0 && teams.Count == 0 && !isRetraction ||
            isRetraction && (resultEvent.Participants.Count != 0 || teams.Count != 0))
        {
            return "Result event must include participants or teams; retractions must be empty";
        }
        if (definition is not null)
        {
            if (ResultDefinition(definition) is not null || resultEvent.DefinitionVersion != definition.Version ||
                !definition.UnitTypes.Contains(resultEvent.UnitType, StringComparer.Ordinal))
            {
                return "Result event definition version or unit type is undeclared";
            }
        }

        var playerIds = new HashSet<string>(StringComparer.Ordinal);
        foreach (var participant in resultEvent.Participants)
        {
            if (participant is null || HasAdditionalProperties(participant.AdditionalProperties) ||
                !ValidIdentifier(participant.PlayerId) ||
                participant.TeamId is not null && !ValidIdentifier(participant.TeamId) ||
                !playerIds.Add(participant.PlayerId) ||
                !ValidOutcomeAndRank(participant.Outcome, participant.Rank) ||
                ResultMetrics(participant.Metrics, "participant", resultEvent.UnitType, definition) is not null)
            {
                return "Result participant is invalid";
            }
        }
        var teamIds = new HashSet<string>(StringComparer.Ordinal);
        foreach (var team in teams)
        {
            if (team is null || HasAdditionalProperties(team.AdditionalProperties) ||
                !ValidIdentifier(team.TeamId) || !teamIds.Add(team.TeamId) ||
                !ValidOutcomeAndRank(team.Outcome, team.Rank) ||
                ResultMetrics(team.Metrics, "team", resultEvent.UnitType, definition) is not null)
            {
                return "Result team is invalid";
            }
        }
        if (teamIds.Count > 0 &&
            resultEvent.Participants.Any(participant =>
                participant.TeamId is not null && !teamIds.Contains(participant.TeamId)))
        {
            return "Participant references an undeclared team";
        }

        try
        {
            if (JsonSerializer.SerializeToUtf8Bytes(resultEvent, RoomHostJson.HostOptions).Length > 256 * 1024)
            {
                return "Result event exceeds the 256 KiB serialized limit";
            }
        }
        catch (JsonException)
        {
            return "Result event is not valid JSON";
        }
        return null;
    }

    public static string? ResultUnits(
        IReadOnlyList<GameResultEvent>? resultUnits,
        GameResultDefinition? definition = null)
    {
        if (resultUnits is null)
        {
            return null;
        }
        if (resultUnits.Count > 32)
        {
            return "Result units exceed the 32-event per-transition limit";
        }
        var eventIds = new HashSet<string>(StringComparer.Ordinal);
        var unitIds = new HashSet<(string MatchId, string UnitId)>();
        foreach (var resultEvent in resultUnits)
        {
            if (ResultEvent(resultEvent, definition) is string error)
            {
                return error;
            }
            if (!eventIds.Add(resultEvent.EventId) || !unitIds.Add((resultEvent.MatchId, resultEvent.UnitId)))
            {
                return "Result units contain duplicate event or unit IDs";
            }
        }
        return null;
    }

    public static string? MatchConfig(MatchConfig? config, GameManifest manifest, string gameId)
    {
        if (config is null)
        {
            return "Match configuration is required";
        }

        if (string.IsNullOrWhiteSpace(config.MatchId) ||
            string.IsNullOrWhiteSpace(config.GameId) ||
            !string.Equals(config.GameId, gameId, StringComparison.Ordinal))
        {
            return "Match and game identifiers are invalid";
        }

        if (config.Players is null ||
            config.Players.Count < manifest.MinPlayers ||
            config.Players.Count > manifest.MaxPlayers)
        {
            return "Match player count is outside the manifest limits";
        }

        var players = new HashSet<string>(StringComparer.Ordinal);
        foreach (var playerId in config.Players)
        {
            if (string.IsNullOrWhiteSpace(playerId) || !players.Add(playerId))
            {
                return "Match players must have unique, non-empty identifiers";
            }
        }

        if (config.MatchSeq < 1 || config.MatchSeq > MaxSafeInteger)
        {
            return "matchSeq must be a positive safe integer";
        }

        if (!IsDefined(config.Carryover))
        {
            return "carryover is required";
        }

        if (config.TickRateHz is double tickRateHz &&
            (!double.IsFinite(tickRateHz) || tickRateHz is < 1 or > 60))
        {
            return "tickRateHz must be between 1 and 60";
        }

        if (IsDefined(config.Options) && config.Options.ValueKind != JsonValueKind.Object)
        {
            return "options must be an object";
        }

        if (IsDefined(config.Options) &&
            config.Options.TryGetProperty("seed", out var seed) &&
            (seed.ValueKind != JsonValueKind.Number ||
             !seed.TryGetDouble(out var seedValue) ||
             !double.IsFinite(seedValue)))
        {
            return "options.seed must be a finite JSON number";
        }

        return null;
    }

    public static string? Tick(TickRequest? request)
    {
        if (request is null || request.Ctx is null)
        {
            return "ctx is required";
        }

        if (!IsDefined(request.State))
        {
            return "state is required";
        }

        var context = request.Ctx;
        if (context.Tick < 0 || context.Tick > MaxSafeInteger)
        {
            return "ctx.tick must be a non-negative safe integer";
        }

        if (!double.IsFinite(context.ElapsedSeconds) || context.ElapsedSeconds < 0d)
        {
            return "ctx.elapsedSeconds must be finite and non-negative";
        }

        if (context.Inputs is null)
        {
            return "ctx.inputs must be an array";
        }

        foreach (var input in context.Inputs)
        {
            if (input is null ||
                string.IsNullOrWhiteSpace(input.PlayerId) ||
                !IsDefined(input.Data) ||
                input.Timestamp is long timestamp &&
                (timestamp < 0 || timestamp > MaxSafeInteger))
            {
                return "Tick inputs must use valid player IDs, JSON data, and non-negative safe timestamps";
            }
        }

        if (context.Commands is not null)
        {
            foreach (var command in context.Commands)
            {
                if (command is null || string.IsNullOrWhiteSpace(command.Type) || !IsDefined(command.Data))
                {
                    return "Tick commands are invalid";
                }
            }
        }

        return null;
    }

    public static string? PlayerEvent(PlayerEventRequest? request)
    {
        if (request is null || string.IsNullOrWhiteSpace(request.PlayerId))
        {
            return "playerId is required";
        }

        return IsDefined(request.State) ? null : "state is required";
    }

    public static string? GameOver(IsGameOverRequest? request) =>
        request is not null && IsDefined(request.State) ? null : "state is required";

    public static string? Command(CommandRequest? request)
    {
        if (request is null || request.Command is null || string.IsNullOrWhiteSpace(request.Command.Type))
        {
            return "command.type is required";
        }

        if (!IsDefined(request.Command.Data))
        {
            return "command.data is required";
        }

        return IsDefined(request.State) ? null : "state is required";
    }

    public static string? InitResult(InitResult? result) =>
        result is null ? "Callback returned no result" :
            StateAndBroadcasts(result.State, result.Broadcasts) ?? ResultUnits(result.ResultUnits);

    public static string? TickResult(TickResult? result) =>
        result is null ? "Callback returned no result" :
            StateAndBroadcasts(result.State, result.Broadcasts) ?? ResultUnits(result.ResultUnits);

    public static string? PlayerEventResult(PlayerEventResult? result) =>
        result is null ? "Callback returned no result" :
            StateAndBroadcasts(result.State, result.Broadcasts) ?? ResultUnits(result.ResultUnits);

    public static string? CommandResult(CommandResult? result) =>
        result is null ? "Callback returned no result" :
            StateAndBroadcasts(result.State, result.Broadcasts) ?? ResultUnits(result.ResultUnits);

    public static string? GameResult(GameResult? result)
    {
        if (result is null)
        {
            return "Callback returned no result";
        }

        if (ResultUnits(result.ResultUnits) is string resultUnitsError)
        {
            return resultUnitsError;
        }

        if (!result.Over)
        {
            return null;
        }

        if (!IsDefined(result.Summary) || result.NextMatchPolicy is null)
        {
            return "Game-over results require summary and nextMatchPolicy";
        }

        if (result.Summary.ValueKind != JsonValueKind.Object ||
            !result.Summary.TryGetProperty("durationSeconds", out var duration) ||
            duration.ValueKind != JsonValueKind.Number ||
            !duration.TryGetDouble(out var durationSeconds) ||
            !double.IsFinite(durationSeconds) || durationSeconds < 0d)
        {
            return "Game-over summary durationSeconds must be finite and non-negative";
        }

        var policy = result.NextMatchPolicy;
        if (policy.Mode is not ("auto_next_match" or "wait_for_ready" or "close_room") ||
            policy.TimeoutMs < 0)
        {
            return "Game-over nextMatchPolicy is invalid";
        }

        return null;
    }

    private static string? StateAndBroadcasts(JsonElement state, JsonElement broadcasts)
    {
        if (!IsDefined(state))
        {
            return "Callback result state is required";
        }

        if (IsDefined(broadcasts) && broadcasts.ValueKind != JsonValueKind.Array)
        {
            return "Callback result broadcasts must be an array";
        }

        return null;
    }

    private static bool IsDefined(JsonElement value) => value.ValueKind != JsonValueKind.Undefined;

    private static string? ResultMetrics(
        IReadOnlyDictionary<string, double>? metrics,
        string scope,
        string unitType,
        GameResultDefinition? definition)
    {
        if (metrics is null || metrics.Count > 128)
        {
            return "Result metrics are invalid";
        }
        foreach (var (id, value) in metrics)
        {
            if (!ValidIdentifier(id) || !double.IsFinite(value))
            {
                return "Result metric identifier or value is invalid";
            }
            if (definition is not null)
            {
                var metric = definition.Metrics.FirstOrDefault(candidate => candidate.Id == id);
                if (metric is null || metric.Scope != scope || !metric.UnitTypes.Contains(unitType, StringComparer.Ordinal) ||
                    !IsDirectAggregation(metric.Aggregation))
                {
                    return $"Metric {id} is not a declared direct {scope} metric";
                }
            }
        }
        return null;
    }

    private static bool ValidOutcomeAndRank(string? outcome, long? rank) =>
        (outcome is null || outcome is "win" or "loss" or "draw" or "push") &&
        (rank is null || rank is > 0 and <= MaxSafeInteger);

    private static bool UniqueIdentifiers(IReadOnlyList<string>? values, out HashSet<string> unique)
    {
        unique = new HashSet<string>(StringComparer.Ordinal);
        if (values is null || values.Count == 0)
        {
            return false;
        }
        foreach (var value in values)
        {
            if (!ValidIdentifier(value) || !unique.Add(value))
            {
                return false;
            }
        }
        return true;
    }

    private static bool ValidIdentifier(string? value) =>
        !string.IsNullOrWhiteSpace(value) && value.EnumerateRunes().Count() <= 128;

    private static bool HasAdditionalProperties(Dictionary<string, JsonElement>? properties) =>
        properties is { Count: > 0 };

    private static bool IsSimpleAggregation(JsonElement aggregation, string expected) =>
        aggregation.ValueKind == JsonValueKind.String &&
        aggregation.GetString() == expected;

    private static bool IsDirectAggregation(JsonElement aggregation) =>
        aggregation.ValueKind == JsonValueKind.String &&
        aggregation.GetString() is "sum" or "min" or "max";

    private static bool TryReadAggregation(
        JsonElement aggregation,
        out string kind,
        out string numeratorId,
        out string denominatorId)
    {
        kind = string.Empty;
        numeratorId = string.Empty;
        denominatorId = string.Empty;
        if (aggregation.ValueKind == JsonValueKind.String)
        {
            kind = aggregation.GetString() ?? string.Empty;
            return true;
        }
        if (aggregation.ValueKind != JsonValueKind.Object)
        {
            return false;
        }
        var properties = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        foreach (var property in aggregation.EnumerateObject())
        {
            if (!properties.TryAdd(property.Name, property.Value))
            {
                return false;
            }
        }
        if (properties.Count != 3 ||
            !properties.TryGetValue("type", out var type) ||
            !properties.TryGetValue("numeratorMetricId", out var numerator) ||
            !properties.TryGetValue("denominatorMetricId", out var denominator) ||
            type.ValueKind != JsonValueKind.String ||
            numerator.ValueKind != JsonValueKind.String ||
            denominator.ValueKind != JsonValueKind.String)
        {
            return false;
        }
        kind = type.GetString() ?? string.Empty;
        numeratorId = numerator.GetString() ?? string.Empty;
        denominatorId = denominator.GetString() ?? string.Empty;
        return kind == "ratio" && ValidIdentifier(numeratorId) && ValidIdentifier(denominatorId);
    }

}
