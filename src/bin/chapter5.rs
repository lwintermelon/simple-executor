#![allow(unused)]
#![feature(negative_impls)]

use std::{cell::UnsafeCell, mem::MaybeUninit, sync::Arc, thread};

fn main() {}

pub mod oneshot_channel {
    use std::{
        collections::VecDeque,
        sync::{atomic::Ordering, Condvar, Mutex},
    };

    /// a struct for testing Drop.
    #[derive(Debug, PartialEq)]
    pub struct MyStr<'a> {
        str: &'a str,
    }

    impl Drop for MyStr<'_> {
        fn drop(&mut self) {
            println!("MyStr:{self:?} drop is called.");
        }
    }

    impl<'a> From<&'a str> for MyStr<'a> {
        fn from(value: &'a str) -> Self {
            Self { str: value }
        }
    }

    pub mod mutex_based_channel {
        use std::thread;

        use super::*;
        pub struct Channel<T> {
            queue: Mutex<VecDeque<T>>,
            item_ready: Condvar,
        }

        impl<T> Channel<T> {
            pub fn new() -> Self {
                Self {
                    queue: Mutex::new(VecDeque::new()),
                    item_ready: Condvar::new(),
                }
            }
            pub fn send(&self, v: T) {
                self.queue.lock().unwrap().push_back(v);
                self.item_ready.notify_one();
            }
            pub fn receive(&self) -> T {
                let mut guard = self.queue.lock().unwrap();
                loop {
                    if let Some(message) = guard.pop_front() {
                        return message;
                    }
                    guard = self.item_ready.wait(guard).unwrap();
                }
            }
        }
    }

    pub mod safety_through_runtime_checks {
        use std::{
            cell::UnsafeCell,
            mem::MaybeUninit,
            sync::atomic::{
                AtomicBool, AtomicU8,
                Ordering::{Acquire, Relaxed, Release},
            },
        };

        const EMPTY: u8 = 0u8;
        const WRITING: u8 = 1u8;
        const READY: u8 = 2u8;
        const READING: u8 = 3u8;
        pub struct Channel<T> {
            message: UnsafeCell<MaybeUninit<T>>,
            state: AtomicU8,
        }
        unsafe impl<T> Sync for Channel<T> where T: Send {}
        impl<T> Channel<T> {
            pub const fn new() -> Self {
                Self {
                    message: UnsafeCell::new(MaybeUninit::uninit()),
                    state: AtomicU8::new(0u8),
                }
            }

            // Safety: Only call this once!
            pub fn send(&self, message: T) {
                if let Err(_) = self
                    .state
                    .compare_exchange(EMPTY, WRITING, Relaxed, Relaxed)
                {
                    panic!("can't send more than one message!");
                }
                unsafe {
                    (*self.message.get()).write(message);
                }
                return self.state.store(READY, Release);
            }

            pub fn is_ready(&self) -> bool {
                self.state.load(Relaxed) == READY
            }

            /// Panics if no message is available yet,
            /// or if the message was already consumed.
            ///
            /// Tip: Use `is_ready` to check first.
            pub fn receive(&self) -> T {
                match self
                    .state
                    .compare_exchange(READY, READING, Acquire, Relaxed)
                {
                    Ok(_) => {
                        return unsafe { (*self.message.get()).assume_init_read() };
                    }
                    Err(_) => {
                        panic!("no message available!")
                    }
                }

                // Safety: We've just checked (and reset) the ready flag.
            }
        }

        impl<T> Drop for Channel<T> {
            fn drop(&mut self) {
                if *self.state.get_mut() == READY {
                    unsafe {
                        self.message.get_mut().assume_init_drop();
                    }
                }
            }
        }
        #[cfg(test)]
        pub mod test {
            use std::thread;

            use crate::oneshot_channel::MyStr;

            use super::*;

            #[test]
            pub fn test() {
                let channel = Channel::new();
                let t = thread::current();
                thread::scope(|s| {
                    s.spawn(|| {
                        channel.send(MyStr::from("hello world!"));
                        t.unpark();
                    });
                    while !channel.is_ready() {
                        thread::park();
                    }
                    assert_eq!(channel.receive(), "hello world!".into());
                });
            }
        }
    }

    pub mod safety_through_types {
        use std::{
            cell::UnsafeCell,
            mem::MaybeUninit,
            sync::{
                atomic::{
                    fence, AtomicBool,
                    Ordering::{Acquire, Relaxed, Release},
                },
                Arc,
            },
        };
        pub struct Sender<T> {
            channel: Arc<Channel<T>>,
        }

        pub struct Receiver<T> {
            channel: Arc<Channel<T>>,
        }

        struct Channel<T> {
            message: UnsafeCell<MaybeUninit<T>>,
            ready: AtomicBool,
        }

        unsafe impl<T> Sync for Channel<T> where T: Send {}

        pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
            let channel = Arc::new(Channel {
                message: UnsafeCell::new(MaybeUninit::uninit()),
                ready: AtomicBool::new(false),
            });
            let sender = Sender {
                channel: channel.clone(),
            };
            let receiver = Receiver { channel };
            (sender, receiver)
        }

        impl<T> Sender<T> {
            pub fn send(self, message: T) {
                unsafe {
                    let p = self.channel.message.get();
                    (*self.channel.message.get()).write(message);
                }
                self.channel.ready.store(true, Release);
            }
        }

        impl<T> Receiver<T> {
            pub fn is_ready(&self) -> bool {
                self.channel.ready.load(Relaxed)
            }
            pub fn receive(self) -> T {
                if !self.channel.ready.swap(false, Relaxed) {
                    panic!("no message available!");
                }
                fence(std::sync::atomic::Ordering::Acquire);
                unsafe { (*self.channel.message.get()).assume_init_read() }
            }
        }

        impl<T> Drop for Channel<T> {
            fn drop(&mut self) {
                if *self.ready.get_mut() {
                    unsafe { (*self.message.get()).assume_init_drop() };
                }
            }
        }

        #[cfg(test)]
        pub mod test {
            use std::thread;

            use super::*;
            #[test]
            pub fn test() {
                let (sender, receiver) = channel();
                let t = thread::current();
                thread::scope(|s| {
                    s.spawn(move || {
                        sender.send("hello world!");
                        t.unpark();
                    });
                    while !receiver.is_ready() {
                        thread::park();
                    }
                    let message = receiver.receive();
                    assert_eq!(message, "hello world!");
                });
            }
        }
    }

    pub mod borrowing_to_avoid_allocation {
        use std::{
            cell::UnsafeCell,
            mem::MaybeUninit,
            sync::{
                atomic::{
                    fence, AtomicBool,
                    Ordering::{Acquire, Relaxed, Release},
                },
                Arc,
            },
            thread::{self, Thread},
        };

        pub struct Sender<'a, T> {
            channel: &'a Channel<T>,
            receiving_thread: Thread,
        }

        pub struct Receiver<'a, T> {
            channel: &'a Channel<T>,
        }
        impl<T> !Send for Receiver<'_, T> {}
        pub struct Channel<T> {
            message: UnsafeCell<MaybeUninit<T>>,
            ready: AtomicBool,
        }

        unsafe impl<T> Sync for Channel<T> where T: Send {}

        impl<T> Channel<T> {
            pub const fn new() -> Self {
                Self {
                    message: UnsafeCell::new(MaybeUninit::uninit()),
                    ready: AtomicBool::new(false),
                }
            }

            pub fn split<'a>(&'a mut self) -> (Sender<'a, T>, Receiver<'a, T>) {
                // if the mutable borrow expires, the caller is allowed to call this again.
                // In such situation, We just reuse the memory of *self by resetting everything to the begainning,
                // and if previous borrows send a message but not be received, drop the message.
                // Translating this to rust, it's `*self = Self::new()`, creating a new Self,
                // then moving old *self out and moving a new one in, the old will be droped.
                *self = Self::new();
                (
                    Sender {
                        channel: self,
                        receiving_thread: thread::current(),
                    },
                    Receiver { channel: self },
                )
            }
        }

        impl<T> Sender<'_, T> {
            pub fn send(self, message: T) {
                unsafe {
                    let p = self.channel.message.get();
                    (*self.channel.message.get()).write(message);
                }
                self.channel.ready.store(true, Release);
                self.receiving_thread.unpark();
            }
        }

        impl<T> Receiver<'_, T> {
            pub fn is_ready(&self) -> bool {
                self.channel.ready.load(Relaxed)
            }
            pub fn receive(self) -> T {
                while !self.channel.ready.swap(false, Relaxed) {
                    thread::park();
                }
                fence(std::sync::atomic::Ordering::Acquire);
                unsafe { (*self.channel.message.get()).assume_init_read() }
            }
        }

        impl<T> Drop for Channel<T> {
            fn drop(&mut self) {
                if *self.ready.get_mut() {
                    unsafe { (*self.message.get()).assume_init_drop() };
                }
            }
        }

        #[cfg(test)]
        pub mod test {
            use std::thread;

            use super::*;
            #[test]
            pub fn test() {
                let mut channel: Channel<&str> = Channel::new();

                thread::scope(|s| {
                    let (sender, receiver) = channel.split();

                    s.spawn(move || {
                        sender.send("hello world!");
                    });
                    let message = receiver.receive();
                    assert_eq!(message, "hello world!");
                });
            }
        }
    }
}
