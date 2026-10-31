import type {
  PlatformSqlResponse,
  PlatformBroadcastResponse,
  PlatformSendResponse,
  PlatformPlayersResponse,
  PlatformKvResponse,
  PlatformKvMutationResponse,
} from "./types.js";

export interface PlatformClientOptions {
  fetch?: typeof fetch;
  vhostMap?: Record<string, string>;
}

export class PlatformClient {
  private readonly customFetch: typeof fetch;
  private readonly vhostMap: Record<string, string>;

  constructor(options: PlatformClientOptions = {}) {
    this.customFetch = options.fetch || globalThis.fetch;
    this.vhostMap = options.vhostMap || {};
  }

  private resolveUrl(vhost: string, path: string): string {
    const base = this.vhostMap[vhost] || `http://${vhost}`;
    const cleanPath = path.startsWith("/") ? path : `/${path}`;
    return `${base}${cleanPath}`;
  }

  async sql(query: string, params: unknown[] = []): Promise<Array<Record<string, unknown>>> {
    if (typeof query !== "string") {
      throw new Error("SQL query must be a string.");
    }
    if (!Array.isArray(params)) {
      throw new Error("SQL parameters must be an array.");
    }

    const response = await this.customFetch(this.resolveUrl("platform.db", "/sql"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sql: query, params }),
    });
    if (!response.ok) {
      throw new Error(`Platform SQL query failed with HTTP ${response.status}.`);
    }

    const data = (await response.json()) as PlatformSqlResponse;
    return data.rows || [];
  }

  async broadcast(message: unknown): Promise<{ success: boolean; recipientCount: number }> {
    if (message === undefined || message === null) {
      throw new Error("A broadcast message is required.");
    }

    const response = await this.customFetch(this.resolveUrl("platform.broadcast", "/"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message }),
    });
    if (!response.ok) {
      throw new Error(`Platform broadcast failed with HTTP ${response.status}.`);
    }

    const data = (await response.json()) as PlatformBroadcastResponse;
    return {
      success: Boolean(data.success),
      recipientCount: data.recipientCount ?? 0,
    };
  }

  async send(playerId: string, message: unknown): Promise<{ success: boolean; delivered: boolean }> {
    if (typeof playerId !== "string" || playerId.trim().length === 0) {
      throw new Error("A non-empty playerId is required.");
    }
    if (message === undefined || message === null) {
      throw new Error("A direct message is required.");
    }

    const response = await this.customFetch(this.resolveUrl("platform.send", "/"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ playerId, message }),
    });
    if (!response.ok) {
      throw new Error(`Platform direct message failed with HTTP ${response.status}.`);
    }

    const data = (await response.json()) as PlatformSendResponse;
    return {
      success: Boolean(data.success),
      delivered: Boolean(data.delivered),
    };
  }

  async getPlayers(): Promise<Array<{ playerId: string; joinedAt: number }>> {
    const response = await this.customFetch(this.resolveUrl("platform.players", "/"), {
      method: "GET",
    });
    if (!response.ok) {
      throw new Error(`Could not retrieve online players (HTTP ${response.status}).`);
    }

    const data = (await response.json()) as PlatformPlayersResponse;
    return data.players || [];
  }

  async kvGet<T = unknown>(key: string): Promise<T | null> {
    if (typeof key !== "string" || key.length === 0) {
      throw new Error("A non-empty KV key is required.");
    }

    const response = await this.customFetch(
      this.resolveUrl("platform.kv", `/${encodeURIComponent(key)}`),
      { method: "GET" },
    );
    if (!response.ok) {
      throw new Error(`Could not read the KV value (HTTP ${response.status}).`);
    }

    const data = (await response.json()) as PlatformKvResponse<T>;
    return data.value !== undefined ? data.value : null;
  }

  async kvPut(key: string, value: unknown): Promise<boolean> {
    if (typeof key !== "string" || key.length === 0) {
      throw new Error("A non-empty KV key is required.");
    }

    const response = await this.customFetch(
      this.resolveUrl("platform.kv", `/${encodeURIComponent(key)}`),
      {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ value }),
      },
    );
    if (!response.ok) {
      throw new Error(`Could not write the KV value (HTTP ${response.status}).`);
    }

    const data = (await response.json()) as PlatformKvMutationResponse;
    return Boolean(data.success);
  }

  async kvDelete(key: string): Promise<boolean> {
    if (typeof key !== "string" || key.length === 0) {
      throw new Error("A non-empty KV key is required.");
    }

    const response = await this.customFetch(
      this.resolveUrl("platform.kv", `/${encodeURIComponent(key)}`),
      { method: "DELETE" },
    );
    if (!response.ok) {
      throw new Error(`Could not delete the KV value (HTTP ${response.status}).`);
    }

    const data = (await response.json()) as PlatformKvMutationResponse;
    return Boolean(data.success);
  }
}
