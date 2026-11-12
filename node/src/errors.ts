export const SDK_ERROR_METADATA = {
  INVALID_REQUEST: {
    retryable: false,
    creatorAction: "Correct the request JSON and required fields before retrying.",
  },
  INVALID_STREAM_MESSAGE: {
    retryable: false,
    creatorAction: "Correct the Session Stream message shape and field types.",
  },
  INVALID_MANIFEST: {
    retryable: false,
    creatorAction: "Correct the game manifest fields and restart the game server.",
  },
  INVALID_OPTIONS: {
    retryable: false,
    creatorAction: "Correct match options to match the declared options schema.",
  },
  INVALID_STATE: {
    retryable: false,
    creatorAction: "Correct the game state shape and version before replaying or resuming.",
  },
  WRONG_ROSTER: {
    retryable: false,
    creatorAction: "Use only active roster player IDs and provide exactly one view per active seat.",
  },
  INVALID_ACTION: {
    retryable: false,
    creatorAction: "Correct the action payload and action identifiers to match the declared action schema.",
  },
  TICK_SEQUENCE_MISMATCH: {
    retryable: false,
    creatorAction: "Send the next contiguous tick after the session's current tick.",
  },
  REPLAY_MISMATCH: {
    retryable: false,
    creatorAction: "Restore a checkpoint and replay each committed tick exactly once in order.",
  },
  CALLBACK_FAILED: {
    retryable: false,
    creatorAction: "Fix the game callback and redeploy; callback internals are intentionally not exposed.",
  },
  INVALID_RESULT: {
    retryable: false,
    creatorAction: "Correct the game callback result to satisfy its documented schema.",
  },
  INVALID_GAME_OVER_POLICY: {
    retryable: false,
    creatorAction: "Return a terminal result with a supported next-match policy and non-negative integer timeout.",
  },
  RESOURCE_LIMIT_EXCEEDED: {
    retryable: false,
    creatorAction: "Reduce request, stream frame, state, result, or view size below the documented limit.",
  },
  UNSUPPORTED_FEATURE: {
    retryable: false,
    creatorAction: "Remove the unsupported feature or implement its required callback.",
  },
  NOT_FOUND: {
    retryable: false,
    creatorAction: "Correct the endpoint or resource identifier.",
  },
  INTERNAL_ERROR: {
    retryable: true,
    creatorAction: "Retry only after resolving a transient server or runtime failure.",
  },
} as const;

export type SdkErrorCode = keyof typeof SDK_ERROR_METADATA;

export class SdkProtocolError extends Error {
  constructor(
    readonly code: SdkErrorCode,
    message: string = SDK_ERROR_METADATA[code].creatorAction,
  ) {
    super(message);
    this.name = "SdkProtocolError";
  }

  get retryable(): boolean {
    return SDK_ERROR_METADATA[this.code].retryable;
  }
}
