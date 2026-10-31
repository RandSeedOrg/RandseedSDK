namespace Randseed.GameServerSdk;

public interface IGameServer
{
    GameManifest GetManifest();

    ValueTask<InitResult> InitAsync(MatchConfig config, CancellationToken cancellationToken);

    ValueTask<TickResult> TickAsync(TickRequest request, CancellationToken cancellationToken);

    ValueTask<PlayerEventResult> OnPlayerJoinAsync(
        PlayerEventRequest request,
        CancellationToken cancellationToken);

    ValueTask<PlayerEventResult> OnPlayerLeaveAsync(
        PlayerEventRequest request,
        CancellationToken cancellationToken);

    ValueTask<GameResult> IsGameOverAsync(
        IsGameOverRequest request,
        CancellationToken cancellationToken);

    ValueTask<CommandResult> OnCommandAsync(
        CommandRequest request,
        CancellationToken cancellationToken);
}
