#![allow(unused)]

use std::{
    sync::atomic::{
        AtomicBool,
        Ordering::{Acquire, Release},
    },
    thread::{self, scope},
};

pub mod spink_lock {
    pub use super::*;
    pub mod mininal {
        use super::*;
        pub struct SpinLock {
            locked: AtomicBool,
        }

        impl SpinLock {
            pub fn new() -> Self {
                Self {
                    locked: AtomicBool::new(false),
                }
            }
            pub fn lock(&self) {
                while self.locked.swap(true, Acquire) {
                    std::hint::spin_loop();
                }
            }
            pub fn unlock(&self) {
                self.locked.store(false, Release);
            }
        }
    }

    pub mod guard {
        use super::*;
        use std::{cell::UnsafeCell, sync::atomic::AtomicBool};
        pub struct SpinLock<T> {
            locked: AtomicBool,
            value: UnsafeCell<T>,
        }
        unsafe impl<T> Sync for SpinLock<T> where T: Send {}
        impl<T> SpinLock<T> {
            pub fn new(value: T) -> Self {
                Self {
                    locked: AtomicBool::new(false),
                    value: UnsafeCell::new(value),
                }
            }

            pub fn lock(&'_ self) -> Guard<'_, T> {
                while self.locked.swap(true, Acquire) {
                    std::hint::spin_loop();
                }
                Guard { lock: self }
            }
        }

        pub struct Guard<'a, T> {
            lock: &'a SpinLock<T>,
        }
        use std::ops::{Deref, DerefMut};
        impl<T> Deref for Guard<'_, T> {
            type Target = T;

            fn deref(&self) -> &T {
                // Safety: The very existence of this Guard
                // guarantees we've exclusively locked the lock.
                unsafe { &*self.lock.value.get() }
            }
        }

        impl<T> DerefMut for Guard<'_, T> {
            // Safety: The very existence of this Guard
            // guarantees we've exclusively locked the lock.
            fn deref_mut(&mut self) -> &mut T {
                unsafe { &mut *self.lock.value.get() }
            }
        }

        unsafe impl<T> Send for Guard<'_, T> where T: Send {}
        unsafe impl<T> Sync for Guard<'_, T> where T: Sync {}
        impl<T> Drop for Guard<'_, T> {
            fn drop(&mut self) {
                self.lock.locked.store(false, Release);
            }
        }
    }
}

fn main() {
    let x = spink_lock::guard::SpinLock::new(Vec::new());
    thread::scope(|s| {
        s.spawn(|| {
            x.lock().push(1);
        });
        s.spawn(|| {
            let mut g = x.lock();
            g.push(2);
            g.push(3);
        });
    });
    let g = x.lock();
    assert!(g.as_slice() == [1, 2, 3] || g.as_slice() == [2, 3, 1])
}
