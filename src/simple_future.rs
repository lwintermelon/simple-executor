use atomic_waker::AtomicWaker;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
    thread,
    time::Duration,
};

// a simple leaf future
pub struct TimerFuture {
    shared_state: Arc<SharedState>,
}

struct SharedState {
    completed: AtomicBool,
    waker: AtomicWaker,
}

impl Future for TimerFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.shared_state.completed.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            self.shared_state.waker.register(&cx.waker());
            Poll::Pending
        }
    }
}

impl TimerFuture {
    pub fn new(duration: Duration) -> Self {
        let shared_state = Arc::new(SharedState {
            completed: AtomicBool::new(false),
            waker: AtomicWaker::new(),
        });
        let thread_shared_state = shared_state.clone();
        thread::spawn(move || {
            thread::sleep(duration);
            thread_shared_state.completed.store(true, Ordering::Release);
            thread_shared_state.waker.wake();
        });
        TimerFuture { shared_state }
    }
}
