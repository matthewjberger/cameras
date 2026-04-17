use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock a [`Mutex`] and recover from poisoning by taking the inner value.
///
/// Used crate-wide so a panicked writer never permanently disables a mutex.
pub(crate) fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
