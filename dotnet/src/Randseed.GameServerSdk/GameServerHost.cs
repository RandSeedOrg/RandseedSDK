using System.Diagnostics;
using System.Text.Json;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Hosting;

namespace Randseed.GameServerSdk;

public static class GameServerHost
{
    public const long MaxRequestBodyBytes = 1_048_576;

    public static WebApplication Create(
        IGameServer gameServer,
        string gameId,
        GameServerHostOptions? options = null,
        string[]? args = null)
    {
        ArgumentNullException.ThrowIfNull(gameServer);
        ArgumentException.ThrowIfNullOrWhiteSpace(gameId);

        var hostOptions = options ?? new GameServerHostOptions();
        if (!Uri.TryCreate(hostOptions.Url, UriKind.Absolute, out var url) ||
            (url.Scheme != Uri.UriSchemeHttp && url.Scheme != Uri.UriSchemeHttps))
        {
            throw new ArgumentException("Url must be an absolute HTTP or HTTPS URL.", nameof(options));
        }

        var builder = WebApplication.CreateBuilder(args ?? []);
        builder.WebHost.UseUrls(hostOptions.Url);
        builder.WebHost.ConfigureKestrel(serverOptions =>
            serverOptions.Limits.MaxRequestBodySize = MaxRequestBodyBytes);

        var app = builder.Build();
        var uptime = Stopwatch.StartNew();

        app.Use(async (context, next) =>
        {
            try
            {
                if (context.Request.ContentLength is long contentLength &&
                    contentLength > MaxRequestBodyBytes)
                {
                    await WriteErrorAsync(context, StatusCodes.Status413PayloadTooLarge, "Request body too large");
                    return;
                }

                await next(context);
            }
            catch (BadHttpRequestException exception)
            {
                var statusCode = exception.StatusCode == StatusCodes.Status413PayloadTooLarge
                    ? StatusCodes.Status413PayloadTooLarge
                    : StatusCodes.Status400BadRequest;
                var message = statusCode == StatusCodes.Status413PayloadTooLarge
                    ? "Request body too large"
                    : "Invalid request";
                await WriteErrorIfPossibleAsync(context, statusCode, message);
            }
            catch (Exception) when (!context.RequestAborted.IsCancellationRequested)
            {
                await WriteErrorIfPossibleAsync(
                    context,
                    StatusCodes.Status500InternalServerError,
                    "Internal server error");
            }
        });

        app.MapGet("/manifest", () => GetManifest(gameServer));
        app.MapGet("/health", () =>
        {
            var manifestError = GetManifestError(gameServer, out _);
            if (manifestError is not null)
            {
                return Error(StatusCodes.Status500InternalServerError, "Internal server error");
            }

            return Results.Json(
                new HealthResponse("ok", gameId, uptime.Elapsed.TotalSeconds),
                RoomHostJson.HostOptions);
        });

        app.MapPost("/init", (Delegate)((HttpContext context) => HandleJsonAsync<MatchConfig, InitResult>(
            context,
            config => RequestValidation.MatchConfig(config, GetValidManifest(gameServer), gameId),
            (config, cancellationToken) => gameServer.InitAsync(config, cancellationToken),
            RequestValidation.InitResult)));

        app.MapPost("/tick", (Delegate)((HttpContext context) => HandleJsonAsync<TickRequest, TickResult>(
            context,
            RequestValidation.Tick,
            (request, cancellationToken) =>
            {
                var normalized = request.Ctx.Commands is null
                    ? request with { Ctx = request.Ctx with { Commands = Array.Empty<HostCommand>() } }
                    : request;
                return gameServer.TickAsync(normalized, cancellationToken);
            },
            RequestValidation.TickResult)));

        app.MapPost("/onPlayerJoin", (Delegate)((HttpContext context) => HandleJsonAsync<PlayerEventRequest, PlayerEventResult>(
            context,
            RequestValidation.PlayerEvent,
            gameServer.OnPlayerJoinAsync,
            RequestValidation.PlayerEventResult)));

        app.MapPost("/onPlayerLeave", (Delegate)((HttpContext context) => HandleJsonAsync<PlayerEventRequest, PlayerEventResult>(
            context,
            RequestValidation.PlayerEvent,
            gameServer.OnPlayerLeaveAsync,
            RequestValidation.PlayerEventResult)));

        app.MapPost("/isGameOver", (Delegate)((HttpContext context) => HandleJsonAsync<IsGameOverRequest, GameResult>(
            context,
            RequestValidation.GameOver,
            gameServer.IsGameOverAsync,
            RequestValidation.GameResult)));

        app.MapPost("/onCommand", (Delegate)((HttpContext context) => HandleJsonAsync<CommandRequest, CommandResult>(
            context,
            RequestValidation.Command,
            gameServer.OnCommandAsync,
            RequestValidation.CommandResult)));

        return app;
    }

    private static IResult GetManifest(IGameServer gameServer)
    {
        try
        {
            var manifest = gameServer.GetManifest();
            return RequestValidation.Manifest(manifest) is null
                ? Results.Json(manifest, RoomHostJson.HostOptions)
                : Error(StatusCodes.Status500InternalServerError, "Internal server error");
        }
        catch (Exception)
        {
            return Error(StatusCodes.Status500InternalServerError, "Internal server error");
        }
    }

    private static string? GetManifestError(IGameServer gameServer, out GameManifest? manifest)
    {
        try
        {
            manifest = gameServer.GetManifest();
            return RequestValidation.Manifest(manifest);
        }
        catch (Exception)
        {
            manifest = null;
            return "Internal server error";
        }
    }

    private static GameManifest GetValidManifest(IGameServer gameServer)
    {
        var error = GetManifestError(gameServer, out var manifest);
        if (error is not null || manifest is null)
        {
            throw new InvalidOperationException("Game manifest is unavailable");
        }

        return manifest;
    }

    private static async Task<IResult> HandleJsonAsync<TRequest, TResponse>(
        HttpContext context,
        Func<TRequest?, string?> validateRequest,
        Func<TRequest, CancellationToken, ValueTask<TResponse>> execute,
        Func<TResponse?, string?> validateResponse)
        where TRequest : class
        where TResponse : class
    {
        if (!context.Request.HasJsonContentType())
        {
            return Error(StatusCodes.Status400BadRequest, "Expected an application/json request body");
        }

        try
        {
            var request = await context.Request.ReadFromJsonAsync<TRequest>(
                RoomHostJson.HostOptions,
                context.RequestAborted);
            if (request is null)
            {
                return Error(StatusCodes.Status400BadRequest, "Request body must be a JSON object");
            }

            var requestError = validateRequest(request);
            if (requestError is not null)
            {
                return Error(StatusCodes.Status400BadRequest, requestError);
            }

            var response = await execute(request, context.RequestAborted);
            if (validateResponse(response) is not null)
            {
                return Error(StatusCodes.Status500InternalServerError, "Internal server error");
            }

            return Results.Json(response, RoomHostJson.HostOptions);
        }
        catch (JsonException)
        {
            return Error(StatusCodes.Status400BadRequest, "Invalid JSON request body");
        }
        catch (BadHttpRequestException exception)
        {
            var statusCode = exception.StatusCode == StatusCodes.Status413PayloadTooLarge
                ? StatusCodes.Status413PayloadTooLarge
                : StatusCodes.Status400BadRequest;
            return Error(
                statusCode,
                statusCode == StatusCodes.Status413PayloadTooLarge
                    ? "Request body too large"
                    : "Invalid request");
        }
        catch (OperationCanceledException) when (context.RequestAborted.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception)
        {
            return Error(StatusCodes.Status500InternalServerError, "Internal server error");
        }
    }

    private static IResult Error(int statusCode, string message) =>
        Results.Json(new ErrorResponse(message), RoomHostJson.HostOptions, statusCode: statusCode);

    private static async Task WriteErrorIfPossibleAsync(HttpContext context, int statusCode, string message)
    {
        if (context.Response.HasStarted || context.RequestAborted.IsCancellationRequested)
        {
            return;
        }

        context.Response.Clear();
        context.Response.StatusCode = statusCode;
        await context.Response.WriteAsJsonAsync(
            new ErrorResponse(message),
            RoomHostJson.HostOptions,
            cancellationToken: context.RequestAborted);
    }

    private static Task WriteErrorAsync(HttpContext context, int statusCode, string message) =>
        WriteErrorIfPossibleAsync(context, statusCode, message);
}
