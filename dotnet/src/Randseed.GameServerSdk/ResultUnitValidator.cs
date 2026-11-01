namespace Randseed.GameServerSdk;

/// <summary>Validates Result Unit declarations and events against protocol limits.</summary>
public static class ResultUnitValidator
{
    /// <summary>Returns null when the versioned result definition is valid.</summary>
    public static string? ValidateDefinition(GameResultDefinition? definition) =>
        RequestValidation.ResultDefinition(definition);

    /// <summary>Returns null when the event is valid for its optional definition.</summary>
    public static string? ValidateEvent(
        GameResultEvent? resultEvent,
        GameResultDefinition? definition = null) =>
        RequestValidation.ResultEvent(resultEvent, definition);

    /// <summary>Returns null when the transition's optional event batch is valid.</summary>
    public static string? ValidateEvents(
        IReadOnlyList<GameResultEvent>? resultUnits,
        GameResultDefinition? definition = null) =>
        RequestValidation.ResultUnits(resultUnits, definition);
}
