// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Browser runtime for arti (`tor_rtcompat` traits): tasks on the page's event loop
//! (`spawn_local`), timers from `setTimeout`, and "blocking" work run inline (there are no
//! threads). Combined with [`crate::net::BridgeNet`] and [`crate::tls::TorTls`] in a
//! `CompoundRuntime`.
//!
//! `Send`/`Sync`: arti requires them of runtimes and their futures. `wasm32-unknown-unknown`
//! without the `atomics` target feature has exactly one thread, so values never cross threads;
//! the one `unsafe impl` below states that (and the build refuses a threaded target).

#[cfg(target_feature = "atomics")]
compile_error!("Tor mode's browser runtime assumes a single-threaded wasm32 target");

use futures::task::{FutureObj, Spawn, SpawnError};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tor_rtcompat::{Blocking, CoarseInstant, CoarseTimeProvider, RealCoarseTimeProvider, SleepProvider};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// Spawner, sleeper and blocking-work runner of the browser runtime.
#[derive(Clone, Debug, Default)]
pub struct WebTask {
    coarse: RealCoarseTimeProvider,
}

impl Spawn for WebTask {
    fn spawn_obj(&self, future: FutureObj<'static, ()>) -> Result<(), SpawnError> {
        wasm_bindgen_futures::spawn_local(future);
        Ok(())
    }
}

/// Resolves an inline computation (the value is ready when the future is created).
pub struct Ready<T>(Option<T>);

impl<T> Future for Ready<T> {
    type Output = T;
    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<T> {
        Poll::Ready(self.0.take().expect("polled after completion"))
    }
}

impl<T> Unpin for Ready<T> {}

impl Blocking for WebTask {
    type ThreadHandle<T: Send + 'static> = Ready<T>;

    fn spawn_blocking<F, T>(&self, f: F) -> Ready<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        // No threads in a page: CPU work (e.g. onion-service proof of work) runs inline.
        Ready(Some(f()))
    }

    fn reenter_block_on<F>(&self, _future: F) -> F::Output
    where
        F: Future,
        F::Output: Send + 'static,
    {
        panic!("Tor mode: block_on is impossible in a browser");
    }
}

impl CoarseTimeProvider for WebTask {
    fn now_coarse(&self) -> CoarseInstant {
        self.coarse.now_coarse()
    }
}

// ---- timers ----

struct TimerState {
    done: bool,
    waker: Option<Waker>,
}

thread_local! {
    /// Live `setTimeout` callbacks by timer id (dropped when they fire or are cancelled).
    static TIMERS: RefCell<HashMap<i32, Closure<dyn FnMut()>>> = RefCell::new(HashMap::new());
}

/// A `setTimeout` timer as a future; cancelled when dropped.
pub struct Sleep {
    state: Arc<Mutex<TimerState>>,
    id: i32,
}

// SAFETY: single-threaded target (see the module docs and the compile_error! above).
unsafe impl Send for Sleep {}

impl Sleep {
    fn new(d: Duration) -> Self {
        let state = Arc::new(Mutex::new(TimerState { done: false, waker: None }));
        let st = state.clone();
        let id_cell = Arc::new(Mutex::new(0i32));
        let id_for_cb = id_cell.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
            s.done = true;
            if let Some(w) = s.waker.take() {
                w.wake();
            }
            let id = *id_for_cb.lock().unwrap_or_else(|e| e.into_inner());
            // Drop our own closure after this call returns.
            wasm_bindgen_futures::spawn_local(async move {
                TIMERS.with(|t| t.borrow_mut().remove(&id));
            });
        });
        let ms = d.as_millis().min(i32::MAX as u128) as i32;
        let id = web_sys::window()
            .expect("Tor mode runs in a window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), ms)
            .expect("setTimeout");
        *id_cell.lock().unwrap_or_else(|e| e.into_inner()) = id;
        TIMERS.with(|t| t.borrow_mut().insert(id, cb));
        Self { state, id }
    }
}

impl Future for Sleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.done {
            return Poll::Ready(());
        }
        s.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        let done = self.state.lock().map(|s| s.done).unwrap_or(true);
        if !done {
            if let Some(w) = web_sys::window() {
                w.clear_timeout_with_handle(self.id);
            }
            TIMERS.with(|t| t.borrow_mut().remove(&self.id));
        }
    }
}

impl SleepProvider for WebTask {
    type SleepFuture = Sleep;

    fn sleep(&self, duration: Duration) -> Sleep {
        Sleep::new(duration)
    }
}

/// `await`able pause for our own tasks.
pub fn sleep_ms(ms: u32) -> Sleep {
    Sleep::new(Duration::from_millis(ms as u64))
}
