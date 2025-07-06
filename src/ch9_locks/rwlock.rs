use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU32, Ordering},
    u32,
};

use atomic_wait::wait;

pub struct Rwlock<T> {
    // The number of reader locks times tow, plus one if there is a writer waiting
    // u32::MAX if write-locked.
    // This means that readers may acquire the lock when the reader_counters is even,
    // but need to block when odd.
    reader_counters: AtomicU32,
    // Incremented to wake up writers.
    writer_wake_counter: AtomicU32,
    value: UnsafeCell<T>,
}

unsafe impl<T> Sync for Rwlock<T> where T: Send + Sync {}

impl<T> Rwlock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            reader_counters: AtomicU32::new(0),
            value: UnsafeCell::new(value),
            writer_wake_counter: AtomicU32::new(0),
        }
    }

    pub fn read(&self) -> ReadGuard<'_, T> {
        let mut current_reader_counters = self.reader_counters.load(Ordering::Relaxed);

        loop {
            if current_reader_counters % 2 == 0 {
                assert!(current_reader_counters < u32::MAX - 2, "too many readers");
                match self.reader_counters.compare_exchange(
                    current_reader_counters,
                    current_reader_counters + 2,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return ReadGuard { rwlock: self },
                    Err(v) => current_reader_counters = v,
                }
            }

            if current_reader_counters % 2 == 1 {
                atomic_wait::wait(&self.reader_counters, current_reader_counters);
                current_reader_counters = self.reader_counters.load(Ordering::Relaxed);
            }
        }
    }

    pub fn write(&self) -> WriteGuard<'_, T> {
        let mut current_reader_counters = self.reader_counters.load(Ordering::Relaxed);

        loop {
            // no readers, we may get the lock.
            if current_reader_counters <= 1 {
                match self.reader_counters.compare_exchange(
                    current_reader_counters,
                    u32::MAX,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return WriteGuard { rwlock: self },
                    Err(v) => {
                        current_reader_counters = v;
                        continue;
                    }
                }
            }

            // there are readers, marked we have writers.
            if current_reader_counters % 2 == 0 {
                if let Err(v) = self.reader_counters.compare_exchange(
                    current_reader_counters,
                    current_reader_counters + 1,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    current_reader_counters = v;
                    continue;
                }
            }

            // Wait, if it's still locked.
            let current_writer_wake_counter = self.writer_wake_counter.load(Ordering::Acquire);
            current_reader_counters = self.reader_counters.load(Ordering::Relaxed);
            if current_reader_counters > 2 {
                wait(&self.writer_wake_counter, current_writer_wake_counter);
                current_reader_counters = self.reader_counters.load(Ordering::Relaxed);
            }
        }
    }
}

pub struct ReadGuard<'a, T> {
    rwlock: &'a Rwlock<T>,
}
pub struct WriteGuard<'a, T> {
    rwlock: &'a Rwlock<T>,
}

impl<T> Deref for ReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.rwlock.value.get() }
    }
}

impl<T> Deref for WriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.rwlock.value.get() }
    }
}

impl<T> DerefMut for WriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.rwlock.value.get() }
    }
}

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        if self.rwlock.reader_counters.fetch_sub(2, Ordering::Release) == 3 {
            self.rwlock
                .writer_wake_counter
                .fetch_and(1, Ordering::Release);
            atomic_wait::wake_one(&self.rwlock.writer_wake_counter);
        }
    }
}

impl<T> Drop for WriteGuard<'_, T> {
    fn drop(&mut self) {
        self.rwlock.reader_counters.store(0, Ordering::Release);
        atomic_wait::wake_one(&self.rwlock.writer_wake_counter);
        atomic_wait::wake_all(&self.rwlock.reader_counters);
    }
}
