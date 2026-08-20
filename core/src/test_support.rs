//! Test-only instrumentation shared by unit tests.

use std::cell::Cell;

thread_local! {
    static CLASSIFY_CALLS: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn record_classify_call() {
    CLASSIFY_CALLS.with(|count| count.set(count.get() + 1));
}

pub(crate) fn classify_call_count() -> usize {
    CLASSIFY_CALLS.with(Cell::get)
}
