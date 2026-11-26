use randseed_game_sdk::{run_serial_benchmark, SerialBenchmarkError};
use std::cell::Cell;
use std::convert::Infallible;
use std::rc::Rc;
use std::sync::Arc;

#[tokio::test]
async fn accepts_one_and_one_hundred_samples() {
    let one = run_serial_benchmark(|| async { Ok::<_, Infallible>(17) }, 1)
        .await
        .unwrap();
    assert_eq!(one.sample_count, 1);
    assert!(one.p50_ms.is_finite());
    assert!(one.p95_ms.is_finite());

    let calls = Rc::new(Cell::new(0));
    let callback_calls = Rc::clone(&calls);
    let hundred = run_serial_benchmark(
        move || {
            callback_calls.set(callback_calls.get() + 1);
            async { Ok::<_, Infallible>(()) }
        },
        100,
    )
    .await
    .unwrap();

    assert_eq!(hundred.sample_count, 100);
    assert_eq!(calls.get(), 100);
}

#[tokio::test]
async fn rejects_sample_counts_outside_the_supported_bounds_without_calling_back() {
    for sample_count in [0, 101] {
        let calls = Rc::new(Cell::new(0));
        let callback_calls = Rc::clone(&calls);
        let result = run_serial_benchmark(
            move || {
                callback_calls.set(callback_calls.get() + 1);
                async { Ok::<_, Infallible>(()) }
            },
            sample_count,
        )
        .await;

        assert!(matches!(
            result,
            Err(SerialBenchmarkError::InvalidSampleCount {
                sample_count: count
            }) if count == sample_count
        ));
        assert_eq!(calls.get(), 0);
    }
}

#[tokio::test]
async fn awaits_each_async_callback_before_starting_the_next() {
    let active = Rc::new(Cell::new(0));
    let maximum_active = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let callback_active = Rc::clone(&active);
    let callback_maximum = Rc::clone(&maximum_active);
    let callback_calls = Rc::clone(&calls);

    let result = run_serial_benchmark(
        move || {
            let active = Rc::clone(&callback_active);
            let maximum_active = Rc::clone(&callback_maximum);
            let calls = Rc::clone(&callback_calls);
            async move {
                calls.set(calls.get() + 1);
                active.set(active.get() + 1);
                maximum_active.set(maximum_active.get().max(active.get()));
                tokio::task::yield_now().await;
                active.set(active.get() - 1);
                Ok::<_, Infallible>(())
            }
        },
        5,
    )
    .await
    .unwrap();

    assert_eq!(result.sample_count, 5);
    assert_eq!(calls.get(), 5);
    assert_eq!(maximum_active.get(), 1);
    assert_eq!(active.get(), 0);
}

struct DropTracked(Rc<Cell<usize>>);

impl Drop for DropTracked {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[tokio::test]
async fn drops_each_successful_output_before_starting_the_next_sample() {
    let calls = Rc::new(Cell::new(0));
    let dropped_outputs = Rc::new(Cell::new(0));
    let callback_calls = Rc::clone(&calls);
    let callback_dropped_outputs = Rc::clone(&dropped_outputs);

    let result = run_serial_benchmark(
        move || {
            let call = callback_calls.get() + 1;
            assert_eq!(callback_dropped_outputs.get(), call - 1);
            callback_calls.set(call);
            let dropped_outputs = Rc::clone(&callback_dropped_outputs);
            async move { Ok::<_, Infallible>(DropTracked(dropped_outputs)) }
        },
        3,
    )
    .await
    .unwrap();

    assert_eq!(result.sample_count, 3);
    assert_eq!(calls.get(), 3);
    assert_eq!(dropped_outputs.get(), 3);
}

#[derive(Debug)]
struct CallbackFailure;

#[tokio::test]
async fn preserves_callback_error_and_stops_sampling() {
    let failure = Arc::new(CallbackFailure);
    let expected_failure = Arc::clone(&failure);
    let calls = Rc::new(Cell::new(0));
    let callback_calls = Rc::clone(&calls);

    let result = run_serial_benchmark(
        move || {
            let call = callback_calls.get() + 1;
            callback_calls.set(call);
            let failure = Arc::clone(&failure);
            async move {
                if call == 2 {
                    Err(failure)
                } else {
                    Ok(())
                }
            }
        },
        4,
    )
    .await;

    match result {
        Err(SerialBenchmarkError::Callback(error)) => {
            assert!(Arc::ptr_eq(&error, &expected_failure));
        }
        other => panic!("expected the original callback error, got {other:?}"),
    }
    assert_eq!(calls.get(), 2);
}
