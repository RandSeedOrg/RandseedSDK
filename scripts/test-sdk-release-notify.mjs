import assert from "node:assert/strict";
import { test } from "node:test";
import { requestWithRetry } from "./notify-gamecreator-release.mjs";

test("retries transient server failures with bounded exponential backoff", async () => {
  const delays = [];
  let attempts = 0;
  const response = await requestWithRetry(
    "https://gamecreator.example/api/release",
    { method: "POST" },
    "notification",
    {
      fetchImpl: async () => {
        attempts += 1;
        return new Response("", { status: attempts === 1 ? 503 : 202 });
      },
      sleepFn: async delay => delays.push(delay),
    },
  );
  assert.equal(response.status, 202);
  assert.equal(attempts, 2);
  assert.deepEqual(delays, [1000]);
});

test("honors Retry-After on 429 and refreshes the request options for each attempt", async () => {
  const delays = [];
  const authorizations = [];
  let tokenNumber = 0;
  let attempts = 0;
  const response = await requestWithRetry(
    "https://gamecreator.example/api/release",
    async () => ({
      headers: { Authorization: `Bearer token-${++tokenNumber}` },
      method: "POST",
    }),
    "notification",
    {
      fetchImpl: async (_url, options) => {
        attempts += 1;
        authorizations.push(options.headers.Authorization);
        return attempts === 1
          ? new Response("", { status: 429, headers: { "Retry-After": "3" } })
          : new Response("", { status: 202 });
      },
      sleepFn: async delay => delays.push(delay),
    },
  );
  assert.equal(response.status, 202);
  assert.deepEqual(authorizations, ["Bearer token-1", "Bearer token-2"]);
  assert.deepEqual(delays, [3000]);
});

test("retries transport errors but fails immediately for permanent client errors", async () => {
  const delays = [];
  let attempts = 0;
  await requestWithRetry(
    "https://gamecreator.example/api/release",
    {},
    "notification",
    {
      fetchImpl: async () => {
        attempts += 1;
        if (attempts === 1) throw new TypeError("temporary network failure");
        return new Response("", { status: 200 });
      },
      sleepFn: async delay => delays.push(delay),
    },
  );
  assert.equal(attempts, 2);
  assert.deepEqual(delays, [1000]);

  attempts = 0;
  await assert.rejects(
    requestWithRetry(
      "https://gamecreator.example/api/release",
      {},
      "notification",
      {
        fetchImpl: async () => {
          attempts += 1;
          return new Response("", { status: 401 });
        },
        sleepFn: async delay => delays.push(delay),
      },
    ),
    /HTTP 401 after 1 attempt/,
  );
  assert.equal(attempts, 1);
});

test("stops after the configured attempt limit and rejects excessive Retry-After delays", async () => {
  const delays = [];
  let attempts = 0;
  await assert.rejects(
    requestWithRetry(
      "https://gamecreator.example/api/release",
      {},
      "notification",
      {
        fetchImpl: async () => {
          attempts += 1;
          return new Response("", { status: 503 });
        },
        sleepFn: async delay => delays.push(delay),
      },
    ),
    /HTTP 503 after 5 attempt/,
  );
  assert.equal(attempts, 5);
  assert.deepEqual(delays, [1000, 2000, 4000, 8000]);

  attempts = 0;
  await assert.rejects(
    requestWithRetry(
      "https://gamecreator.example/api/release",
      {},
      "notification",
      {
        fetchImpl: async () => {
          attempts += 1;
          return new Response("", { status: 429, headers: { "Retry-After": "3601" } });
        },
        sleepFn: async delay => delays.push(delay),
      },
    ),
    /longer than 15 minutes/,
  );
  assert.equal(attempts, 1);
});
