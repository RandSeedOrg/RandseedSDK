import assert from "node:assert/strict";
import test from "node:test";
import { notifyGamecreatorGuide } from "./notify-gamecreator-guide.mjs";

const sourceSha = "0123456789abcdef0123456789abcdef01234567";

test("notifies the matching Creator Worker with a scoped OIDC token", async () => {
  const requests = [];
  await notifyGamecreatorGuide({
    requestUrl: "https://actions.example/oidc?existing=value",
    requestToken: "runner-token",
    audience: "randseed-gamecreator",
    apiUrl: "https://devcreator.example/",
    sourceSha,
    fetchImpl: async (url, options) => {
      const requestUrl = new URL(url);
      requests.push({ url: requestUrl, options });
      if (requestUrl.pathname === "/oidc") {
        return new Response(JSON.stringify({ value: "short-lived-oidc-token" }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return new Response("", { status: 202 });
    },
  });

  assert.equal(requests.length, 2);
  assert.equal(requests[0].url.searchParams.get("audience"), "randseed-gamecreator");
  assert.equal(requests[0].options.headers.Authorization, "bearer runner-token");
  assert.equal(requests[1].url.href, "https://devcreator.example/api/webhooks/sdk/guide");
  assert.equal(requests[1].options.headers.Authorization, "Bearer short-lived-oidc-token");
  assert.deepEqual(JSON.parse(requests[1].options.body), { sha: sourceSha });
});

test("rejects invalid source SHAs before making network requests", async () => {
  let fetchCount = 0;
  await assert.rejects(
    notifyGamecreatorGuide({
      requestUrl: "https://actions.example/oidc",
      requestToken: "runner-token",
      audience: "randseed-gamecreator",
      apiUrl: "https://devcreator.example",
      sourceSha: "not-a-commit",
      fetchImpl: async () => {
        fetchCount += 1;
        return new Response("", { status: 200 });
      },
    }),
    /source commit SHA is invalid/,
  );
  assert.equal(fetchCount, 0);
});

test("rejects non-HTTPS Creator Worker URLs", async () => {
  await assert.rejects(
    notifyGamecreatorGuide({
      requestUrl: "https://actions.example/oidc",
      requestToken: "runner-token",
      audience: "randseed-gamecreator",
      apiUrl: "http://devcreator.example",
      sourceSha,
    }),
    /must use HTTPS/,
  );
});
