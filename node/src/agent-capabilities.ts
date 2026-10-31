export type AgentControlMode = "copilot" | "autoplay" | "follow";

export interface AgentGameCapabilities {
  observationProtocolVersion: 1;
  actionProtocolVersion: 1;
  modes: AgentControlMode[];
  maxDecisionHz: number;
  backgroundAutoplay: boolean;
}

/**
 * Validate and copy an explicit game declaration. This declaration is not
 * authorization and does not prove that a game constructs safe observations.
 */
export function validateAgentGameCapabilities(value: unknown): AgentGameCapabilities {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Agent capabilities must be an object.");
  }
  const declaration = value as Record<string, unknown>;
  if (declaration.observationProtocolVersion !== 1 || declaration.actionProtocolVersion !== 1) {
    throw new Error("Unsupported Agent protocol version.");
  }
  if (!Array.isArray(declaration.modes) || declaration.modes.length === 0) {
    throw new Error("Agent modes must be non-empty, unique, and supported.");
  }
  const modes = declaration.modes.map((mode: unknown): AgentControlMode => {
    if (mode === "copilot" || mode === "autoplay" || mode === "follow") return mode;
    throw new Error("Agent modes must be non-empty, unique, and supported.");
  });
  if (new Set(modes).size !== modes.length) {
    throw new Error("Agent modes must be non-empty, unique, and supported.");
  }
  if (typeof declaration.maxDecisionHz !== "number" ||
      !Number.isFinite(declaration.maxDecisionHz) || declaration.maxDecisionHz <= 0) {
    throw new Error("maxDecisionHz must be a positive finite number.");
  }
  if (typeof declaration.backgroundAutoplay !== "boolean" ||
      (declaration.backgroundAutoplay && !modes.includes("autoplay"))) {
    throw new Error("Background Autoplay requires Autoplay support.");
  }
  return {
    observationProtocolVersion: 1,
    actionProtocolVersion: 1,
    modes,
    maxDecisionHz: declaration.maxDecisionHz,
    backgroundAutoplay: declaration.backgroundAutoplay,
  };
}
