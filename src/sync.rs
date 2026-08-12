//! Lock helpers that recover a poisoned guard instead of propagating it.
//!
//! Every lock in aibox guards a short critical section over plain data, so a
//! panic while a guard is held cannot leave behind a torn invariant worth
//! propagating. Recovering the guard keeps one unrelated panic from cascading
//! into every later Traffic Proxy task and Docker cleanup path that shares the
//! same lock.

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub(crate) fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn read_unpoisoned<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn write_unpoisoned<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}
