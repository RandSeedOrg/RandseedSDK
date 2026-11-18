import { test } from "node:test";
import assert from "node:assert/strict";
import { runSerialBenchmark } from "../dist/index.js";

test("runSerialBenchmark runs serially and reports nearest-rank p50 and p95", async () => {
  const times = [0, 5, 5, 6, 6, 13, 13, 16, 16, 25];
  let timeIndex = 0;
  let activeCallbacks = 0;
  let maximumConcurrentCallbacks = 0;
  let callbackCount = 0;

  const result = await runSerialBenchmark(
    async () => {
      callbackCount += 1;
      activeCallbacks += 1;
      maximumConcurrentCallbacks = Math.max(maximumConcurrentCallbacks, activeCallbacks);
      await new Promise((resolve) => setImmediate(resolve));
      activeCallbacks -= 1;
    },
    {
      sampleCount: 5,
      now: () => times[timeIndex++],
    },
  );

  assert.deepEqual(result, { sampleCount: 5, p50Ms: 5, p95Ms: 9 });
  assert.equal(callbackCount, 5);
  assert.equal(maximumConcurrentCallbacks, 1);
});

test("runSerialBenchmark uses nearest-rank percentiles for one and two samples", async () => {
  const oneSampleTimes = [10, 14];
  const oneSample = await runSerialBenchmark(() => undefined, {
    sampleCount: 1,
    now: () => oneSampleTimes.shift(),
  });
  assert.deepEqual(oneSample, { sampleCount: 1, p50Ms: 4, p95Ms: 4 });

  const twoSampleTimes = [0, 8, 8, 10];
  const twoSamples = await runSerialBenchmark(() => undefined, {
    sampleCount: 2,
    now: () => twoSampleTimes.shift(),
  });
  assert.deepEqual(twoSamples, { sampleCount: 2, p50Ms: 2, p95Ms: 8 });
});

test("runSerialBenchmark rejects invalid sample counts before invoking the callback", async () => {
  let callbackCount = 0;

  for (const sampleCount of [0, -1, 1.5, 101, Number.NaN, Number.POSITIVE_INFINITY]) {
    await assert.rejects(
      runSerialBenchmark(() => {
        callbackCount += 1;
      }, { sampleCount }),
      RangeError,
    );
  }

  assert.equal(callbackCount, 0);
});

test("runSerialBenchmark propagates callback failures unchanged and stops sampling", async () => {
  const failure = new Error("fixture callback failed");
  let callbackCount = 0;

  await assert.rejects(
    runSerialBenchmark(async () => {
      callbackCount += 1;
      if (callbackCount === 2) throw failure;
    }, { sampleCount: 4 }),
    (error) => error === failure,
  );

  assert.equal(callbackCount, 2);
});

test("runSerialBenchmark rejects a non-monotonic injected clock", async () => {
  const times = [5, 4];

  await assert.rejects(
    runSerialBenchmark(() => undefined, {
      sampleCount: 1,
      now: () => times.shift(),
    }),
    RangeError,
  );
});
