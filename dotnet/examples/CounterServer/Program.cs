using System.Text.Json;
using Microsoft.Extensions.Hosting;
using Randseed.GameServerSdk;

var app = GameServerHost.Create(new CounterGameServer(), "room-counter");
await app.RunAsync();

internal sealed class CounterGameServer : IGameServer
{
    public GameManifest GetManifest() => new()
    {
        Title = "Room Counter",
        Version = "1.0.0",
        MinPlayers = 1,
        MaxPlayers = 4,
        TickRateHz = 5,
        PersistentTable = false
    };

    public ValueTask<InitResult> InitAsync(MatchConfig config, CancellationToken cancellationToken)
    {
        var initialCount = config.Carryover.ValueKind == JsonValueKind.Number &&
            config.Carryover.TryGetInt32(out var carryoverCount)
                ? carryoverCount
                : 0;
        var state = CreateState(initialCount, config.Players);

        return ValueTask.FromResult(new InitResult
        {
            State = state,
            Broadcasts = GameJson.ToElement(new[] { new { type = "started" } })
        });
    }

    public ValueTask<TickResult> TickAsync(TickRequest request, CancellationToken cancellationToken)
    {
        var (count, players) = ReadState(request.State);
        foreach (var input in request.Ctx.Inputs!)
        {
            if (input.Data.ValueKind == JsonValueKind.Object &&
                input.Data.TryGetProperty("delta", out var delta) &&
                delta.TryGetInt32(out var value))
            {
                count = checked(count + value);
            }
        }

        return ValueTask.FromResult(new TickResult
        {
            State = CreateState(count, players),
            Broadcasts = GameJson.ToElement(new[] { new { type = "counter.changed", count } }),
            Persist = true
        });
    }

    public ValueTask<PlayerEventResult> OnPlayerJoinAsync(
        PlayerEventRequest request,
        CancellationToken cancellationToken)
    {
        var (count, players) = ReadState(request.State);
        var updatedPlayers = players.Contains(request.PlayerId, StringComparer.Ordinal)
            ? players
            : players.Append(request.PlayerId).ToArray();

        return ValueTask.FromResult(new PlayerEventResult
        {
            State = CreateState(count, updatedPlayers),
            Broadcasts = GameJson.ToElement(new[] { new { type = "player_joined", playerId = request.PlayerId } })
        });
    }

    public ValueTask<PlayerEventResult> OnPlayerLeaveAsync(
        PlayerEventRequest request,
        CancellationToken cancellationToken)
    {
        var (count, players) = ReadState(request.State);
        var updatedPlayers = players.Where(playerId =>
            !string.Equals(playerId, request.PlayerId, StringComparison.Ordinal)).ToArray();

        return ValueTask.FromResult(new PlayerEventResult
        {
            State = CreateState(count, updatedPlayers),
            Broadcasts = GameJson.ToElement(new[] { new { type = "player_left", playerId = request.PlayerId } })
        });
    }

    public ValueTask<GameResult> IsGameOverAsync(
        IsGameOverRequest request,
        CancellationToken cancellationToken)
    {
        var (count, _) = ReadState(request.State);
        var over = count >= 20;

        return ValueTask.FromResult(new GameResult
        {
            Over = over,
            Summary = over ? GameJson.ToElement(new { finalCount = count }) : default,
            NextMatchPolicy = over
                ? new NextMatchPolicy
                {
                    Mode = "auto_next_match",
                    TimeoutMs = 0,
                    Carryover = GameJson.ToElement(new { count })
                }
                : null
        });
    }

    public ValueTask<CommandResult> OnCommandAsync(
        CommandRequest request,
        CancellationToken cancellationToken)
    {
        var (count, players) = ReadState(request.State);
        if (request.Command.Type == "reset")
        {
            count = request.Command.Data.ValueKind == JsonValueKind.Object &&
                    request.Command.Data.TryGetProperty("count", out var resetCount) &&
                    resetCount.TryGetInt32(out var value)
                ? value
                : 0;
        }

        return ValueTask.FromResult(new CommandResult
        {
            State = CreateState(count, players),
            Broadcasts = GameJson.ToElement(new[] { new { type = "counter.reset" } })
        });
    }

    private static JsonElement CreateState(int count, IReadOnlyList<string> players) =>
        GameJson.ToElement(new { count, players });

    private static (int Count, string[] Players) ReadState(JsonElement state)
    {
        if (state.ValueKind != JsonValueKind.Object)
        {
            return (0, Array.Empty<string>());
        }

        var count = state.TryGetProperty("count", out var countElement) &&
                    countElement.TryGetInt32(out var stateCount)
            ? stateCount
            : 0;
        var players = state.TryGetProperty("players", out var playersElement) &&
                      playersElement.ValueKind == JsonValueKind.Array
            ? playersElement.EnumerateArray()
                .Where(player => player.ValueKind == JsonValueKind.String)
                .Select(player => player.GetString()!)
                .ToArray()
            : Array.Empty<string>();

        return (count, players);
    }
}
