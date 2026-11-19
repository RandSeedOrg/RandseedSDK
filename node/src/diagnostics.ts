import { performance } from "node:perf_hooks";

/** Result of a serial, in-process callback benchmark. */
export interface SerialBenchmarkResult {
  sampleCount: number;
  p50Ms: number;
  p95Ms: number;
}

/**
 * Options for a bounded serial callback benchmark.
 *
 * `sampleCount` must be an integer from 1 through 100. `now` is an optional
 * monotonic clock source intended for deterministic tests; applications should
 * normally omit it.
 */
export interface SerialBenchmarkOptions {
  sampleCount: number;
  now?: () => number;
}

/**
 * Measures one callback repeatedly and serially using a monotonic clock.
 * Callback return values are discarded; callback errors propagate unchanged.
 */
export async function runSerialBenchmark(
  callback: () => unknown,
  options: SerialBenchmarkOptions,
): Promise<SerialBenchmarkResult> {
  if (typeof callback !== "function") {
    throw new TypeError("callback must be a function.");
  }
  if (options === null || typeof options !== "object") {
    throw new TypeError("options must be an object.");
  }

  const { sampleCount, now = () => performance.now() } = options;
  if (!Number.isInteger(sampleCount) || sampleCount < 1 || sampleCount > 100) {
    throw new RangeError("sampleCount must be an integer from 1 through 100.");
  }
  if (typeof now !== "function") {
    throw new TypeError("now must be a function when provided.");
  }

  const durations: number[] = [];
  let previousTime = Number.NEGATIVE_INFINITY;
  const readTime = (): number => {
    const time = now();
    if (!Number.isFinite(time) || time < previousTime) {
      throw new RangeError("now must return finite, monotonically non-decreasing values.");
    }
    previousTime = time;
    return time;
  };

  for (let sample = 0; sample < sampleCount; sample += 1) {
    const startedAt = readTime();
    await callback();
    const finishedAt = readTime();
    durations.push(finishedAt - startedAt);
  }

  durations.sort((left, right) => left - right);
  const nearestRank = (percentile: number): number =>
    durations[Math.ceil(percentile * sampleCount) - 1];

  return {
    sampleCount,
    p50Ms: nearestRank(0.5),
    p95Ms: nearestRank(0.95),
  };
}
