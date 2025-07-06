use futures::task::AtomicWaker;
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
        // quick check to avoid registration if already done.
        if self.shared_state.completed.load(Ordering::Relaxed) {
            return Poll::Ready(());
        }

        self.shared_state.waker.register(&cx.waker()); 

        // Need to check condition **after** `register` to avoid a race
        // condition that would result in lost notifications.
        if self.shared_state.completed.load(Ordering::Relaxed) {
            Poll::Ready(())
        } else {
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
            thread_shared_state.completed.store(true, Ordering::Relaxed);
            thread_shared_state.waker.wake();
        });
        TimerFuture { shared_state }
    }
}
