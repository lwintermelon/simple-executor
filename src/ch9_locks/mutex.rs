use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU32, Ordering::*},
};

pub struct Mutex<T> {
    // 0: unlocked
    // 1: locked, no other thread waiting
    // 2; locked, other thread waiting
    state: AtomicU32,
    value: UnsafeCell<T>,
}

unsafe impl<T> Sync for Mutex<T> where T: Send {}

pub struct MutexGuard<'a, T> {
    pub(crate) mutex: &'a Mutex<T>,
}

unsafe impl<T> Send for MutexGuard<'_, T> where T: Send {}
unsafe impl<T> Sync for MutexGuard<'_, T> where T: Sync {}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            state: AtomicU32::new(0),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        if self.state.compare_exchange(0, 1, Acquire, Relaxed).is_err() {
            Self::lock_contended(&self.state);
        }
        MutexGuard { mutex: self }
    }

    #[cold]
    fn lock_contended(state: &AtomicU32) {
        let mut spin_count = 0;
        while state.load(Relaxed) == 1 && spin_count < 100 {
            spin_count += 1;
            std::hint::spin_loop();
        }

        if state.compare_exchange(0, 1, Acquire, Relaxed).is_ok() {
            return;
        }
        while state.swap(2, Acquire) != 0 {
            atomic_wait::wait(state, 2);
        }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        if self.mutex.state.swap(0, Release) == 2 {
            atomic_wait::wake_one(&self.mutex.state);
        }
    }
}

#[test]
fn test_mutex() {
    let m = Mutex::new(0);
    std::hint::black_box(&m);
    let start = std::time::Instant::now();
    for _ in 0..5_000_000 {
        *m.lock() += 1;
    }
    let duration = start.elapsed();
    println!("locked {} times in {:?}", *m.lock(), duration);
}

#[test]
fn test_mutex_contend() {
    let m = Mutex::new(0);
    std::hint::black_box(&m);
    let start = std::time::Instant::now();
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..5_000_000 {
                    *m.lock() += 1;
                }
            });
        }
    });
    let duration = start.elapsed();
    println!("locked {} times in {:?}", *m.lock(), duration);
}
