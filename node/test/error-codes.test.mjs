import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { SDK_ERROR_METADATA, SdkProtocolError } from "../dist/index.js";

const fixture = JSON.parse(
  await readFile(new URL("../../protocol/fixtures/sdk-error-codes-v1.json", import.meta.url), "utf8"),
);

test("Node SDK error codes, retryability and creator guidance match the shared fixture", () => {
  assert.equal(fixture.protocolVersion, 1);
  assert.deepEqual(
    Object.fromEntries(
      Object.entries(SDK_ERROR_METADATA).map(([code, metadata]) => [
        code,
        { code, ...metadata },
      ]),
    ),
    Object.fromEntries(fixture.errors.map(entry => [entry.code, entry])),
  );
  assert.equal(new SdkProtocolError("CALLBACK_FAILED").retryable, false);
  assert.equal(new SdkProtocolError("INTERNAL_ERROR").retryable, true);
});
