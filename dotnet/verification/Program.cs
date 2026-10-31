using System.Net;
using System.Text;
using System.Text.Json;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting.Server;
using Microsoft.AspNetCore.Hosting.Server.Features;
using Microsoft.AspNetCore.Http.Features;
using Microsoft.Extensions.DependencyInjection;
using Randseed.GameServerSdk;

var fixturePath = Path.Combine(AppContext.BaseDirectory, "protocol", "room-host-v1.json");
using var fixtureDocument = JsonDocument.Parse(await File.ReadAllTextAsync(fixturePath));
var fixture = fixtureDocument.RootElement;
var jsonOptions = RoomHostJson.CreateOptions();

var manifest = Read<GameManifest>("manifest");
var initResult = Read<InitResult>("initResponse");
var tickResult = Read<TickResult>("tickResponse");
var playerEventResult = Read<PlayerEventResult>("playerEventResponse");
var gameResult = Read<GameResult>("gameOverResponse");
var commandResult = Read<CommandResult>("commandResponse");

VerifyMulberryVectors();
await VerifyHttpHostAsync();
Console.WriteLine("All .NET SDK fixture, PRNG, validation, and HTTP serialization checks passed.");

T Read<T>(string property) where T : class =>
    JsonSerializer.Deserialize<T>(fixture.GetProperty(property).GetRawText(), jsonOptions)
    ?? throw new InvalidOperationException($"Fixture property '{property}' was null.");

void VerifyMulberryVectors()
{
    foreach (var vector in fixture.GetProperty("prngVectors").EnumerateArray())
    {
        var random = new Mulberry32(vector.GetProperty("seed").GetDouble());
        var expected = vector.GetProperty("u32").EnumerateArray().Select(value => value.GetUInt32()).ToArray();

        foreach (var value in expected)
        {
            Assert(random.NextUInt32() == value, $"Mulberry32 mismatch for seed {vector.GetProperty("seed")}");
        }

        var firstFloat = new Mulberry32(vector.GetProperty("seed").GetDouble()).NextFloat();
        Assert(firstFloat >= 0d && firstFloat < 1d, "Mulberry32.NextFloat must be in [0, 1)");
    }

    Assert(Mulberry32.ToInt32(2_147_483_648d) == int.MinValue, "ToInt32 signed boundary mismatch");
    Assert(Mulberry32.ToInt32(4_294_967_295d) == -1, "ToInt32 unsigned wrap mismatch");
    Assert(Mulberry32.ToInt32(4_294_967_296d) == 0, "ToInt32 full-wrap mismatch");
    Assert(Mulberry32.ToInt32(-1.9d) == -1, "ToInt32 must truncate toward zero");
    Assert(Mulberry32.ToInt32(double.PositiveInfinity) == 0, "ToInt32 non-finite conversion mismatch");
}

async Task VerifyHttpHostAsync()
{
    var server = new FixtureGameServer(
        manifest,
        initResult,
        tickResult,
        playerEventResult,
        gameResult,
        commandResult);
    await using var app = GameServerHost.Create(
        server,
        "room-counter",
        new GameServerHostOptions
        {
            Url = "http://127.0.0.1:0"
        });

    await app.StartAsync();
    try
    {
        var addresses = app.Services.GetRequiredService<IServer>()
            .Features.Get<IServerAddressesFeature>()?.Addresses
            ?? throw new InvalidOperationException("The HTTP server did not publish its bound address.");
        var baseAddress = new Uri(addresses.Single());
        using var client = new HttpClient { BaseAddress = baseAddress };

        await VerifyGetAsync(client, "/manifest", fixture.GetProperty("manifest"));

        using (var health = await client.GetAsync("/health"))
        {
            Assert(health.StatusCode == HttpStatusCode.OK, "GET /health returned an unexpected status.");
            using var healthJson = JsonDocument.Parse(await health.Content.ReadAsStringAsync());
            Assert(healthJson.RootElement.GetProperty("status").GetString() == "ok", "Health status mismatch.");
            Assert(healthJson.RootElement.GetProperty("gameId").GetString() == "room-counter", "Health gameId mismatch.");
            Assert(healthJson.RootElement.GetProperty("uptime").GetDouble() >= 0d, "Health uptime must be non-negative.");
        }

        await VerifyPostAsync(client, "/init", "config", "initResponse");
        await VerifyPostAsync(client, "/tick", "tickRequest", "tickResponse");
        await VerifyPostAsync(client, "/onPlayerJoin", "playerEventRequest", "playerEventResponse");
        await VerifyPostAsync(client, "/onPlayerLeave", "playerEventRequest", "playerEventResponse");
        await VerifyPostAsync(client, "/isGameOver", "isGameOverRequest", "gameOverResponse");
        await VerifyPostAsync(client, "/onCommand", "commandRequest", "commandResponse");

        foreach (var (options, expected) in new[]
        {
            ("", HttpStatusCode.OK),
            (""","options":{}""", HttpStatusCode.OK),
            (""","options":null""", HttpStatusCode.BadRequest)
        })
        {
            using var response = await PostRawAsync(client, "/init",
                """{"matchId":"m","gameId":"room-counter","players":["player-a"],"matchSeq":1,"carryover":null""" + options + "}");
            Assert(response.StatusCode == expected, $"Unexpected status for options {options}.");
        }

        foreach (var (summary, expectedStatus) in new[]
        {
            ("null", HttpStatusCode.InternalServerError),
            ("[]", HttpStatusCode.InternalServerError),
            ("{}", HttpStatusCode.InternalServerError),
            ("""{"durationSeconds":null}""", HttpStatusCode.InternalServerError),
            ("""{"durationSeconds":"1"}""", HttpStatusCode.InternalServerError),
            ("""{"durationSeconds":-1}""", HttpStatusCode.InternalServerError),
            ("""{"durationSeconds":1e400}""", HttpStatusCode.InternalServerError),
            ("""{"durationSeconds":0}""", HttpStatusCode.OK),
            ("""{"durationSeconds":2.5}""", HttpStatusCode.OK)
        })
        {
            using var summaryJson = JsonDocument.Parse(summary);
            server.GameResultOverride = new GameResult
            {
                Over = true,
                Summary = summaryJson.RootElement.Clone(),
                NextMatchPolicy = new NextMatchPolicy { Mode = "close_room", TimeoutMs = 0 }
            };
            using var response = await PostRawAsync(
                client, "/isGameOver", fixture.GetProperty("isGameOverRequest").GetRawText());
            Assert(response.StatusCode == expectedStatus, $"Unexpected status for summary {summary}.");
            if (expectedStatus == HttpStatusCode.InternalServerError)
            {
                Assert((await response.Content.ReadAsStringAsync()).Contains("Internal server error", StringComparison.Ordinal),
                    "Invalid game-over summary response was not sanitized.");
            }
        }
        server.GameResultOverride = null;

        using (var nullTickRate = await PostRawAsync(client, "/init",
            """{"matchId":"m","gameId":"room-counter","players":["player-a"],"matchSeq":1,"options":{},"carryover":null,"tickRateHz":null}"""))
        {
            Assert(nullTickRate.StatusCode == HttpStatusCode.BadRequest, "Null tickRateHz was accepted.");
        }
        using (var omittedTickRate = await PostRawAsync(client, "/init",
            """{"matchId":"m","gameId":"room-counter","players":["player-a"],"matchSeq":1,"options":{},"carryover":null}"""))
        {
            Assert(omittedTickRate.StatusCode == HttpStatusCode.OK, "Omitted tickRateHz was rejected.");
        }

        using (var safeMatchSeq = await PostRawAsync(
            client,
            "/init",
            """
            {"matchId":"safe-seq","gameId":"room-counter","players":["player-a"],"matchSeq":9007199254740991,"options":{},"carryover":null}
            """))
        {
            Assert(safeMatchSeq.StatusCode == HttpStatusCode.OK, "Maximum safe matchSeq was rejected.");
        }

        using (var zeroMatchSeq = await PostRawAsync(
            client,
            "/init",
            """
            {"matchId":"zero-seq","gameId":"room-counter","players":["player-a"],"matchSeq":0,"options":{},"carryover":null}
            """))
        {
            Assert(zeroMatchSeq.StatusCode == HttpStatusCode.BadRequest, "Zero matchSeq was accepted.");
        }

        using (var unsafeMatchSeq = await PostRawAsync(
            client,
            "/init",
            """
            {"matchId":"unsafe-seq","gameId":"room-counter","players":["player-a"],"matchSeq":9007199254740992,"options":{},"carryover":null}
            """))
        {
            Assert(unsafeMatchSeq.StatusCode == HttpStatusCode.BadRequest, "Unsafe matchSeq was accepted.");
        }

        using (var safeTick = await PostRawAsync(
            client,
            "/tick",
            """
            {"ctx":{"tick":9007199254740991,"inputs":[{"playerId":"p","data":null,"timestamp":9007199254740991}],"elapsedSeconds":0},"state":null}
            """))
        {
            Assert(safeTick.StatusCode == HttpStatusCode.OK, "Maximum safe tick or timestamp was rejected.");
        }

        using (var integralNumberNotation = await PostRawAsync(
            client,
            "/tick",
            """
            {"ctx":{"tick":1e2,"inputs":[{"playerId":"p","data":null,"timestamp":0.0}],"elapsedSeconds":0},"state":null}
            """))
        {
            Assert(integralNumberNotation.StatusCode == HttpStatusCode.OK, "Integral JSON number notation was rejected.");
        }

        using (var fractionalTick = await PostRawAsync(
            client,
            "/tick",
            """
            {"ctx":{"tick":1.5,"inputs":[],"elapsedSeconds":0},"state":null}
            """))
        {
            Assert(fractionalTick.StatusCode == HttpStatusCode.BadRequest, "Fractional tick was accepted.");
        }

        using (var unsafeTick = await PostRawAsync(
            client,
            "/tick",
            """
            {"ctx":{"tick":9007199254740992,"inputs":[],"elapsedSeconds":0},"state":null}
            """))
        {
            Assert(unsafeTick.StatusCode == HttpStatusCode.BadRequest, "Unsafe tick was accepted.");
        }

        using (var unsafeTimestamp = await PostRawAsync(
            client,
            "/tick",
            """
            {"ctx":{"tick":0,"inputs":[{"playerId":"p","data":null,"timestamp":9007199254740992}],"elapsedSeconds":0},"state":null}
            """))
        {
            Assert(unsafeTimestamp.StatusCode == HttpStatusCode.BadRequest, "Unsafe input timestamp was accepted.");
        }

        var arbitraryStateValue = GameJson.ToElement(
            new object?[] { null, new { nested = new object?[] { "value", true, 42 } } });
        server.InitStateOverride = arbitraryStateValue;
        using (var arbitraryJson = await PostRawAsync(
            client,
            "/init",
            """
            {"matchId":"arbitrary-json","gameId":"room-counter","players":["player-a"],"matchSeq":1,"options":{"nested":[null,{"enabled":true}]},"carryover":null}
            """))
        {
            Assert(arbitraryJson.StatusCode == HttpStatusCode.OK, "Arbitrary options/carryover JSON was rejected.");
            using var arbitraryResponse = JsonDocument.Parse(await arbitraryJson.Content.ReadAsStringAsync());
            AssertJsonEqual(
                arbitraryStateValue,
                arbitraryResponse.RootElement.GetProperty("state"),
                "Arbitrary callback state");
        }
        server.InitStateOverride = null;

        using (var arbitraryState = await PostRawAsync(
            client,
            "/isGameOver",
            """{"state":[null,{"nested":[1,true]}]}"""))
        {
            Assert(arbitraryState.StatusCode == HttpStatusCode.OK, "Arbitrary JSON state was rejected.");
        }

        using (var malformed = await PostRawAsync(client, "/init", "{\"matchId\":"))
        {
            Assert(malformed.StatusCode == HttpStatusCode.BadRequest, "Malformed JSON was not rejected.");
            Assert((await malformed.Content.ReadAsStringAsync()).Contains("Invalid JSON", StringComparison.Ordinal),
                "Malformed JSON error response was not sanitized.");
        }

        using (var duplicatePlayers = await PostRawAsync(
            client,
            "/init",
            """
            {"matchId":"match-1","gameId":"room-counter","players":["same","same"],"matchSeq":1,"options":{},"carryover":null}
            """))
        {
            Assert(duplicatePlayers.StatusCode == HttpStatusCode.BadRequest, "Duplicate players were not rejected.");
        }

        using (var atLimit = await PostRawAsync(
            client,
            "/init",
            $"\"{new string('x', (int)GameServerHost.MaxRequestBodyBytes - 2)}\""))
        {
            Assert(atLimit.StatusCode == HttpStatusCode.BadRequest, "A body at the 1 MiB limit was rejected as oversized.");
        }

        using (var oversized = await PostRawAsync(
            client,
            "/init",
            $"\"{new string('x', (int)GameServerHost.MaxRequestBodyBytes - 1)}\""))
        {
            Assert((int)oversized.StatusCode == 413, "Oversized JSON request was not rejected.");
            Assert((await oversized.Content.ReadAsStringAsync()).Contains("Request body too large", StringComparison.Ordinal),
                "Oversized request error response was not sanitized.");
        }

        server.ThrowOnTick = true;
        using var callbackClient = new HttpClient { BaseAddress = baseAddress };
        using (var failedCallback = await PostRawAsync(
            callbackClient,
            "/tick",
            fixture.GetProperty("tickRequest").GetRawText()))
        {
            Assert(failedCallback.StatusCode == HttpStatusCode.InternalServerError, "Callback failure status mismatch.");
            var failureText = await failedCallback.Content.ReadAsStringAsync();
            Assert(failureText.Contains("Internal server error", StringComparison.Ordinal), "Callback error response missing.");
            Assert(!failureText.Contains("private callback detail", StringComparison.Ordinal),
                "Callback exception details leaked to the client.");
        }
        server.ThrowOnTick = false;
        server.ThrowJsonOnTick = true;
        using var callbackJsonClient = new HttpClient { BaseAddress = baseAddress };
        using (var failedJsonCallback = await PostRawAsync(
            callbackJsonClient,
            "/tick",
            fixture.GetProperty("tickRequest").GetRawText()))
        {
            Assert(failedJsonCallback.StatusCode == HttpStatusCode.InternalServerError,
                "A callback JsonException was misclassified as a client error.");
            var failureText = await failedJsonCallback.Content.ReadAsStringAsync();
            Assert(failureText.Contains("Internal server error", StringComparison.Ordinal),
                "Callback JsonException response was not sanitized.");
            Assert(!failureText.Contains("private callback detail", StringComparison.Ordinal),
                "Callback JsonException details leaked to the client.");
        }
    }
    finally
    {
        await app.StopAsync();
    }
}

async Task VerifyGetAsync(HttpClient client, string path, JsonElement expected)
{
    using var response = await client.GetAsync(path);
    Assert(response.StatusCode == HttpStatusCode.OK, $"{path} returned an unexpected status.");
    var body = await response.Content.ReadAsStringAsync();
    using var actual = JsonDocument.Parse(body);
    AssertJsonEqual(expected, actual.RootElement, $"{path} response");
}

async Task VerifyPostAsync(HttpClient client, string path, string requestProperty, string responseProperty)
{
    using var response = await PostRawAsync(client, path, fixture.GetProperty(requestProperty).GetRawText());
    Assert(response.StatusCode == HttpStatusCode.OK, $"{path} returned an unexpected status.");
    var body = await response.Content.ReadAsStringAsync();
    using var actual = JsonDocument.Parse(body);
    AssertJsonEqual(fixture.GetProperty(responseProperty), actual.RootElement, $"{path} response");
}

async Task<HttpResponseMessage> PostRawAsync(HttpClient client, string path, string body)
{
    using var content = new StringContent(body, Encoding.UTF8, "application/json");
    return await client.PostAsync(path, content);
}

void AssertJsonEqual(JsonElement expected, JsonElement actual, string label)
{
    Assert(JsonEquals(expected, actual), $"{label} did not match the shared fixture.\nExpected: {expected}\nActual: {actual}");
}

bool JsonEquals(JsonElement left, JsonElement right)
{
    if (left.ValueKind != right.ValueKind)
    {
        if (left.ValueKind == JsonValueKind.Number && right.ValueKind == JsonValueKind.Number)
        {
            return left.GetDouble() == right.GetDouble();
        }

        return false;
    }

    return left.ValueKind switch
    {
        JsonValueKind.Object => ObjectEquals(left, right),
        JsonValueKind.Array => left.GetArrayLength() == right.GetArrayLength() &&
            left.EnumerateArray().Zip(right.EnumerateArray()).All(pair => JsonEquals(pair.First, pair.Second)),
        JsonValueKind.String => left.GetString() == right.GetString(),
        JsonValueKind.Number => left.GetDouble() == right.GetDouble(),
        JsonValueKind.True or JsonValueKind.False => left.GetBoolean() == right.GetBoolean(),
        JsonValueKind.Null => true,
        _ => false
    };
}

bool ObjectEquals(JsonElement left, JsonElement right)
{
    var leftProperties = left.EnumerateObject().ToDictionary(property => property.Name, property => property.Value);
    var rightProperties = right.EnumerateObject().ToDictionary(property => property.Name, property => property.Value);
    return leftProperties.Count == rightProperties.Count &&
        leftProperties.All(property =>
            rightProperties.TryGetValue(property.Key, out var value) && JsonEquals(property.Value, value));
}

void Assert(bool condition, string message)
{
    if (!condition)
    {
        throw new InvalidOperationException(message);
    }
}

sealed class FixtureGameServer : IGameServer
{
    private readonly GameManifest manifest;
    private readonly InitResult initResult;
    private readonly TickResult tickResult;
    private readonly PlayerEventResult playerEventResult;
    private readonly GameResult gameResult;
    private readonly CommandResult commandResult;

    public FixtureGameServer(
        GameManifest manifest,
        InitResult initResult,
        TickResult tickResult,
        PlayerEventResult playerEventResult,
        GameResult gameResult,
        CommandResult commandResult)
    {
        this.manifest = manifest;
        this.initResult = initResult;
        this.tickResult = tickResult;
        this.playerEventResult = playerEventResult;
        this.gameResult = gameResult;
        this.commandResult = commandResult;
    }

    public bool ThrowOnTick { get; set; }

    public bool ThrowJsonOnTick { get; set; }

    public JsonElement? InitStateOverride { get; set; }

    public GameResult? GameResultOverride { get; set; }

    public GameManifest GetManifest() => manifest;

    public ValueTask<InitResult> InitAsync(MatchConfig config, CancellationToken cancellationToken) =>
        ValueTask.FromResult(InitStateOverride is JsonElement state
            ? new InitResult { State = state, Broadcasts = initResult.Broadcasts }
            : initResult);

    public ValueTask<TickResult> TickAsync(TickRequest request, CancellationToken cancellationToken)
    {
        if (ThrowOnTick)
        {
            throw new InvalidOperationException("private callback detail");
        }
        if (ThrowJsonOnTick)
        {
            throw new JsonException("private callback detail");
        }

        return ValueTask.FromResult(tickResult);
    }

    public ValueTask<PlayerEventResult> OnPlayerJoinAsync(
        PlayerEventRequest request,
        CancellationToken cancellationToken) => ValueTask.FromResult(playerEventResult);

    public ValueTask<PlayerEventResult> OnPlayerLeaveAsync(
        PlayerEventRequest request,
        CancellationToken cancellationToken) => ValueTask.FromResult(playerEventResult);

    public ValueTask<GameResult> IsGameOverAsync(
        IsGameOverRequest request,
        CancellationToken cancellationToken) => ValueTask.FromResult(GameResultOverride ?? gameResult);

    public ValueTask<CommandResult> OnCommandAsync(
        CommandRequest request,
        CancellationToken cancellationToken) => ValueTask.FromResult(commandResult);
}
