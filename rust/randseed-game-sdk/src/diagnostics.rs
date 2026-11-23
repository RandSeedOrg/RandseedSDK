//! Developer-initiated, in-process diagnostics for game callbacks.

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::time::{Duration, Instant};

/// Result of a serial, in-process callback benchmark.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SerialBenchmarkResult {
    /// Number of callback invocations.
    pub sample_count: usize,
    /// Nearest-rank 50th percentile callback duration in milliseconds.
    pub p50_ms: f64,
    /// Nearest-rank 95th percentile callback duration in milliseconds.
    pub p95_ms: f64,
}

/// Error returned when a serial callback benchmark cannot be completed.
#[derive(Debug)]
pub enum SerialBenchmarkError<E> {
    /// The sample count was outside the supported range of 1 through 100.
    InvalidSampleCount {
        /// Requested number of callback invocations.
        sample_count: usize,
    },
    /// The callback failed; the original callback error is retained unchanged.
    Callback(E),
}

impl<E> fmt::Display for SerialBenchmarkError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSampleCount { sample_count } => write!(
                formatter,
                "sample_count must be an integer from 1 through 100 (got {sample_count})"
            ),
            Self::Callback(_) => formatter.write_str("serial benchmark callback failed"),
        }
    }
}

impl<E: Error + 'static> Error for SerialBenchmarkError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSampleCount { .. } => None,
            Self::Callback(error) => Some(error),
        }
    }
}

/// Measures an async callback repeatedly and serially using a monotonic clock.
///
/// The callback is awaited before the next sample starts. Its successful return
/// value is discarded. Sample counts must be between 1 and 100 inclusive. If a
/// callback fails, sampling stops and its original error is returned in
/// [`SerialBenchmarkError::Callback`]. This helper does not log, emit telemetry,
/// access a room or network, or change game state except for effects performed
/// by the supplied callback.
///
/// Durations are measured from immediately before each callback invocation
/// until its future completes. Percentiles use the nearest-rank definition:
/// the one-based rank is `ceil(percentile * sample_count)`.
///
/// # Examples
///
/// Benchmark a deterministic game tick against fixed, local fixture values:
///
/// ```
/// use randseed_game_sdk::{
///     run_serial_benchmark, GameServer, SerialBenchmarkError, SerialBenchmarkResult, SdkError,
///     TickContext,
/// };
/// use serde_json::json;
///
/// async fn benchmark_fixture_tick<G: GameServer>(
///     game: &G,
/// ) -> Result<SerialBenchmarkResult, SerialBenchmarkError<SdkError>> {
///     let context = TickContext {
///         tick: 7,
///         inputs: Vec::new(),
///         commands: Vec::new(),
///         elapsed_seconds: 0.05,
///     };
///     let fixture_state = json!({ "score": 41 });
///
///     run_serial_benchmark(
///         || game.tick(context.clone(), fixture_state.clone()),
///         30,
///     )
///     .await
/// }
/// ```
///
/// Use a deterministic game instance with no external side effects and fixed
/// test fixtures. The helper measures local callback execution only; it does
/// not measure room, host, or network latency.
pub async fn run_serial_benchmark<F, Fut, T, E>(
    mut callback: F,
    sample_count: usize,
) -> Result<SerialBenchmarkResult, SerialBenchmarkError<E>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    if !(1..=100).contains(&sample_count) {
        return Err(SerialBenchmarkError::InvalidSampleCount { sample_count });
    }

    let mut durations_ms = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let started_at = Instant::now();
        let output = callback().await.map_err(SerialBenchmarkError::Callback)?;
        durations_ms.push(finish_sample(output, || started_at.elapsed()));
    }

    Ok(summarize_durations(durations_ms))
}

fn finish_sample<T>(output: T, read_elapsed: impl FnOnce() -> Duration) -> f64 {
    let elapsed_ms = read_elapsed().as_secs_f64() * 1_000.0;
    drop(output);
    elapsed_ms
}

fn summarize_durations(mut durations_ms: Vec<f64>) -> SerialBenchmarkResult {
    let sample_count = durations_ms.len();
    durations_ms.sort_by(f64::total_cmp);

    let nearest_rank = |percentile: usize| {
        let one_based_rank = (percentile * sample_count + 99) / 100;
        durations_ms[one_based_rank - 1]
    };

    SerialBenchmarkResult {
        sample_count,
        p50_ms: nearest_rank(50),
        p95_ms: nearest_rank(95),
    }
}

#[cfg(test)]
mod tests {
    use super::{finish_sample, summarize_durations, SerialBenchmarkResult};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    #[test]
    fn drops_callback_output_after_reading_elapsed_time() {
        struct CheckDrop(Rc<Cell<bool>>);

        impl Drop for CheckDrop {
            fn drop(&mut self) {
                assert!(self.0.get(), "output dropped before elapsed time was read");
            }
        }

        let elapsed_was_read = Rc::new(Cell::new(false));
        let output = CheckDrop(Rc::clone(&elapsed_was_read));
        let elapsed_ms = finish_sample(output, || {
            elapsed_was_read.set(true);
            Duration::from_millis(7)
        });

        assert_eq!(elapsed_ms, 7.0);
    }

    #[test]
    fn summarizes_nearest_rank_percentiles() {
        assert_eq!(
            summarize_durations(vec![9.0, 1.0, 5.0, 3.0, 7.0]),
            SerialBenchmarkResult {
                sample_count: 5,
                p50_ms: 5.0,
                p95_ms: 9.0,
            }
        );
        assert_eq!(
            summarize_durations(vec![8.0, 2.0]),
            SerialBenchmarkResult {
                sample_count: 2,
                p50_ms: 2.0,
                p95_ms: 8.0,
            }
        );
    }
}
