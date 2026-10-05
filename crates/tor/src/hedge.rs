// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! A hedged onion request: when the first attempt has not answered by a deadline, a second one
//! runs beside it on fresh circuits, and whichever succeeds first wins.
//!
//! Why (docs/BOARDS.md G.16.2, B-P11): a hosting tab sometimes never completes the rendezvous for
//! one attempt while the next attempt, with another rendezvous point, answers in seconds. Good
//! attempts usually answer in ≤ 10 s but one took 69 s, so the slow first attempt is kept, not
//! cancelled.

use futures::future::{Either, select};
use std::future::Future;

/// Runs `attempt(fresh)`. If it has not finished when `deadline` fires, runs `attempt(true)` (a new
/// isolation group: new circuits, a new rendezvous point) beside it. The first success wins; a
/// failure waits for the other attempt. A failure before the deadline returns at once (the
/// caller's retry rounds take over).
pub async fn hedged<T, D, F, Fut>(fresh: bool, deadline: D, attempt: F) -> Result<T, String>
where
    D: Future<Output = ()>,
    F: Fn(bool) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let first = attempt(fresh);
    futures::pin_mut!(first, deadline);
    let first = match select(first, deadline).await {
        Either::Left((r, _)) => return r,
        Either::Right((_, first)) => first,
    };
    let second = attempt(true);
    futures::pin_mut!(second);
    match select(first, second).await {
        Either::Left((Ok(v), _)) | Either::Right((Ok(v), _)) => Ok(v),
        Either::Left((Err(_), other)) => other.await,
        Either::Right((Err(_), other)) => other.await,
    }
}

#[cfg(test)]
mod tests {
    use super::hedged;
    use futures::executor::block_on;
    use futures::future::{pending, ready};
    use std::cell::RefCell;

    /// An attempt that never answers when `fresh` is false (the lost rendezvous).
    fn stalls_unless_fresh(fresh: bool) -> futures::future::BoxFuture<'static, Result<&'static str, String>> {
        if fresh { Box::pin(ready(Ok("second"))) } else { Box::pin(pending()) }
    }

    #[test]
    fn a_stalled_attempt_is_hedged_on_fresh_circuits() {
        let tries = RefCell::new(Vec::new());
        let r = block_on(hedged(false, ready(()), |f| {
            tries.borrow_mut().push(f);
            stalls_unless_fresh(f)
        }));
        assert_eq!(r, Ok("second"));
        assert_eq!(*tries.borrow(), [false, true]);
    }

    #[test]
    fn an_answer_before_the_deadline_needs_no_second_attempt() {
        let tries = RefCell::new(0);
        let r = block_on(hedged(false, pending(), |_| {
            *tries.borrow_mut() += 1;
            ready(Ok::<_, String>(1))
        }));
        assert_eq!((r, *tries.borrow()), (Ok(1), 1));
    }

    #[test]
    fn a_failure_before_the_deadline_returns_at_once() {
        let tries = RefCell::new(0);
        let r = block_on(hedged(false, pending(), |_| {
            *tries.borrow_mut() += 1;
            ready(Err::<u8, _>("refused".to_string()))
        }));
        assert_eq!((r, *tries.borrow()), (Err("refused".into()), 1));
    }

    #[test]
    fn the_slow_first_attempt_still_wins_when_the_second_fails() {
        // The 69 s case of B-P11: the hedge fails, then the first attempt answers; it is kept.
        let hedge_failed = std::rc::Rc::new(std::cell::Cell::new(false));
        let r = block_on(hedged(false, ready(()), |fresh| {
            let flag = hedge_failed.clone();
            let f: futures::future::LocalBoxFuture<'static, Result<&'static str, String>> = if fresh {
                Box::pin(async move {
                    flag.set(true);
                    Err("no rendezvous".to_string())
                })
            } else {
                Box::pin(futures::future::poll_fn(move |_| if flag.get() { std::task::Poll::Ready(Ok("first")) } else { std::task::Poll::Pending }))
            };
            f
        }));
        assert_eq!(r, Ok("first"));
    }

    #[test]
    fn both_failing_reports_the_last_failure() {
        // The first attempt is still pending at the deadline, then fails; the hedge fails after it.
        let r = block_on(hedged(false, ready(()), |fresh| {
            let f: futures::future::LocalBoxFuture<'static, Result<u8, String>> =
                if fresh { Box::pin(ready(Err("second".to_string()))) } else { Box::pin(async { futures::pending!(); Err("first".to_string()) }) };
            f
        }));
        assert_eq!(r, Err("second".into()));
    }
}
