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
        result is null ? "Callback returned no result" : StateAndBroadcasts(result.State, result.Broadcasts);

    public static string? TickResult(TickResult? result) =>
        result is null ? "Callback returned no result" : StateAndBroadcasts(result.State, result.Broadcasts);

    public static string? PlayerEventResult(PlayerEventResult? result) =>
        result is null ? "Callback returned no result" : StateAndBroadcasts(result.State, result.Broadcasts);

    public static string? CommandResult(CommandResult? result) =>
        result is null ? "Callback returned no result" : StateAndBroadcasts(result.State, result.Broadcasts);

    public static string? GameResult(GameResult? result)
    {
        if (result is null)
        {
            return "Callback returned no result";
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
}
