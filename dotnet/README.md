# RandSeed .NET Game Server SDK

The .NET 8 SDK hosts a game-server implementation over the RandSeed room-host
HTTP/JSON protocol. It uses only .NET and ASP.NET Core framework libraries.

## Run the counter example

From this directory:

```sh
dotnet run --project examples/CounterServer
```

The example listens on `0.0.0.0:8080` and exposes the protocol endpoints:

- `GET /manifest`, `GET /health`
- `POST /init`, `POST /tick`
- `POST /onPlayerJoin`, `POST /onPlayerLeave`
- `POST /isGameOver`, `POST /onCommand`

The host enforces the protocol's 1 MiB JSON request-body limit and returns
`413` when it is exceeded. Configure a different HTTP URL through
`GameServerHostOptions`. Callback and server exceptions produce a sanitized
`500` response.

## Implement a game

Reference `src/Randseed.GameServerSdk/Randseed.GameServerSdk.csproj` and
implement `IGameServer`. The protocol DTOs use camelCase JSON names. State,
options, carryover, summaries, command data, and broadcasts use `JsonElement`
so game data can be any JSON value without imposing a game-specific schema.
`GameJson.ToElement(value)` creates JSON values with camelCase object fields.

```csharp
using Randseed.GameServerSdk;

var app = GameServerHost.Create(new MyGameServer(), "my-game");
await app.RunAsync();
```

`GameServerHost.Create` listens on port 8080 by default. Supply
`GameServerHostOptions` to override the URL. The host
validates manifests, match configuration, ticks, lifecycle events, commands,
and callback results before returning protocol responses.

Use the supplied state and tick context as the authoritative inputs to game
callbacks. Avoid process-global match state, wall-clock simulation, or
unrecorded external inputs. `Mulberry32` implements the protocol's deterministic
generator and JavaScript signed 32-bit seed conversion; it is not cryptographic.

## Build and verify

```sh
dotnet build Randseed.GameServerSdk.sln
dotnet run --project verification
```

The dependency-light verification executable reads the shared fixture at
`../protocol/fixtures/room-host-v1.json`, checks every PRNG `u32` vector, and
exercises host endpoint serialization, validation, request-size limits, and
sanitized callback errors using an in-process HTTP server.

See the [room-host v1 contract](../protocol/room-host-v1.md) for wire details.
