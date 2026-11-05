const MAX_ATTEMPTS = 5;
const MAX_RETRY_AFTER_MS = 15 * 60 * 1000;
const REQUEST_TIMEOUT_MS = 15 * 1000;

function sleep(delayMs) {
  return new Promise(resolve => setTimeout(resolve, delayMs));
}

function retryDelayMs(retryAfter, attempt) {
  if (typeof retryAfter === "string" && retryAfter.trim()) {
    const seconds = Number(retryAfter);
    if (Number.isFinite(seconds) && seconds >= 0) {
      return seconds * 1000;
    }
    const retryAt = Date.parse(retryAfter);
    if (Number.isFinite(retryAt)) {
      return Math.max(0, retryAt - Date.now());
    }
  }
  return 1000 * (2 ** attempt);
}

function isRetryableStatus(status) {
  return status === 408 || status === 425 || status === 429 || (status >= 500 && status <= 599);
}

export async function requestWithRetry(
  url,
  requestOptions,
  label,
  {
    fetchImpl = globalThis.fetch,
    sleepFn = sleep,
    maxAttempts = MAX_ATTEMPTS,
    timeoutMs = REQUEST_TIMEOUT_MS,
  } = {},
) {
  if (!Number.isInteger(maxAttempts) || maxAttempts < 1) {
    throw new TypeError("maxAttempts must be a positive integer.");
  }

  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    const options = typeof requestOptions === "function"
      ? await requestOptions(attempt)
      : requestOptions;
    let response;
    try {
      response = await fetchImpl(url, {
        ...options,
        signal: AbortSignal.timeout(timeoutMs),
      });
    } catch (error) {
      if (attempt + 1 === maxAttempts) {
        const errorName = error instanceof Error ? error.name : "network error";
        throw new Error(
          `${label} failed after ${maxAttempts} attempts (${errorName}); rerun this GitHub Actions job to retry.`,
        );
      }
      await sleepFn(1000 * (2 ** attempt));
      continue;
    }

    if (response.ok) {
      return response;
    }

    const retryable = isRetryableStatus(response.status);
    const delayMs = retryDelayMs(response.headers.get("retry-after"), attempt);
    await response.body?.cancel();
    if (!retryable || attempt + 1 === maxAttempts) {
      throw new Error(
        `${label} failed with HTTP ${response.status} after ${attempt + 1} attempt(s); rerun this GitHub Actions job to retry.`,
      );
    }
    if (delayMs > MAX_RETRY_AFTER_MS) {
      throw new Error(`${label} requested a retry delay longer than 15 minutes; rerun after that delay.`);
    }
    await sleepFn(delayMs);
  }

  throw new Error(`${label} failed after ${maxAttempts} attempts.`);
}

async function getOidcToken(requestUrl, requestToken, audience) {
  const tokenUrl = new URL(requestUrl);
  tokenUrl.searchParams.set("audience", audience);
  const response = await requestWithRetry(tokenUrl, {
    headers: {
      Authorization: `bearer ${requestToken}`,
      Accept: "application/json",
    },
  }, "GitHub Actions OIDC token request");
  const payload = await response.json();
  if (typeof payload.value !== "string" || payload.value.length === 0) {
    throw new Error("GitHub Actions OIDC token response is invalid.");
  }
  return payload.value;
}

async function main() {
  const requestUrl = process.env.ACTIONS_ID_TOKEN_REQUEST_URL;
  const requestToken = process.env.ACTIONS_ID_TOKEN_REQUEST_TOKEN;
  const audience = process.env.OIDC_AUDIENCE;
  const apiUrl = process.env.API_URL;
  if (!requestUrl || !requestToken || !audience || !apiUrl) {
    throw new Error("GitHub Actions OIDC configuration is missing.");
  }

  const releaseId = Number(process.env.RELEASE_ID);
  if (!Number.isSafeInteger(releaseId) || releaseId <= 0) {
    throw new Error("GitHub Release ID is invalid.");
  }
  const body = {
    repository: { full_name: "RandSeedOrg/RandseedSDK" },
    release: {
      id: releaseId,
      tag_name: process.env.RELEASE_TAG,
      draft: false,
      prerelease: false,
    },
    version: process.env.SDK_VERSION,
  };
  const response = await requestWithRetry(
    `${apiUrl}/api/webhooks/sdk/randseedsdk`,
    async () => ({
      method: "POST",
      headers: {
        Authorization: `Bearer ${await getOidcToken(requestUrl, requestToken, audience)}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    }),
    "Gamecreator release notification",
  );
  await response.body?.cancel();
}

if (process.argv[1] && new URL(import.meta.url).pathname === process.argv[1]) {
  main().catch(error => {
    console.error(error instanceof Error ? error.message : "SDK release notification failed.");
    process.exitCode = 1;
  });
}
