import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { requestWithRetry } from "./notify-gamecreator-release.mjs";

const GIT_SHA_PATTERN = /^[0-9a-f]{40}$/;

async function getOidcToken(requestUrl, requestToken, audience, fetchImpl) {
  const tokenUrl = new URL(requestUrl);
  tokenUrl.searchParams.set("audience", audience);
  const response = await requestWithRetry(
    tokenUrl,
    {
      headers: {
        Authorization: `bearer ${requestToken}`,
        Accept: "application/json",
      },
    },
    "GitHub Actions OIDC token request",
    { fetchImpl },
  );
  const payload = await response.json();
  if (typeof payload.value !== "string" || payload.value.length === 0) {
    throw new Error("GitHub Actions OIDC token response is invalid.");
  }
  return payload.value;
}

export async function notifyGamecreatorGuide({
  requestUrl,
  requestToken,
  audience,
  apiUrl,
  sourceSha,
  fetchImpl = globalThis.fetch,
} = {}) {
  if (!requestUrl || !requestToken || !audience || !apiUrl || !sourceSha) {
    throw new Error("GitHub Actions OIDC configuration is missing.");
  }
  if (!GIT_SHA_PATTERN.test(sourceSha)) {
    throw new Error("RandseedSDK source commit SHA is invalid.");
  }
  const endpoint = new URL("/api/webhooks/sdk/guide", apiUrl);
  if (endpoint.protocol !== "https:") {
    throw new Error("Gamecreator API URL must use HTTPS.");
  }

  const response = await requestWithRetry(
    endpoint,
    async () => ({
      method: "POST",
      headers: {
        Authorization: `Bearer ${await getOidcToken(
          requestUrl,
          requestToken,
          audience,
          fetchImpl,
        )}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ sha: sourceSha }),
    }),
    "Gamecreator SDK guide notification",
    { fetchImpl },
  );
  await response.body?.cancel();
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  notifyGamecreatorGuide({
    requestUrl: process.env.ACTIONS_ID_TOKEN_REQUEST_URL,
    requestToken: process.env.ACTIONS_ID_TOKEN_REQUEST_TOKEN,
    audience: process.env.OIDC_AUDIENCE,
    apiUrl: process.env.API_URL,
    sourceSha: process.env.SOURCE_SHA,
  }).catch(error => {
    console.error(error instanceof Error ? error.message : "SDK guide notification failed.");
    process.exitCode = 1;
  });
}
