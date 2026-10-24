//! Locks shared by every tab of the page's origin, through the browser's Web Locks API: a lock is
//! held by one tab at a time, and released when it is dropped or the tab is closed.
//!
//! Without the API, which a page served over plain HTTP from elsewhere than `localhost` lacks,
//! every lock is taken at once by everyone asking.

use std::{
    cell::RefCell,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use js_sys::{Function, Object, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue, prelude::*};

// The bindings of web-sys need `--cfg=web_sys_unstable_apis`, which every crate building the page
// would have to be given.
#[wasm_bindgen]
extern "C" {
    type LockManager;

    #[wasm_bindgen(method)]
    fn request(this: &LockManager, name: &str, options: &Object, callback: &Function) -> Promise;
}

/// A lock held, until it is dropped.
#[must_use = "the lock is released when dropped"]
#[derive(Debug)]
pub struct Lock {
    /// Ends what the browser holds the lock for; `None` without the API.
    release: Option<Function>,
}

impl Drop for Lock {
    fn drop(&mut self) {
        if let Some(release) = &self.release {
            _ = release.call0(&JsValue::UNDEFINED);
        }
    }
}

/// Takes the lock `name` if no tab holds it, or else returns `None`.
pub async fn try_lock(name: &str) -> Option<Lock> {
    request(name, false).await
}

/// Takes the lock `name` as soon as no tab holds it any longer. Dropping the future gives up
/// waiting.
pub async fn lock(name: &str) -> Lock {
    request(name, true)
        .await
        .expect("a lock waited for is granted")
}

/// What a request for a lock and the browser's answer to it share.
#[derive(Default)]
struct State {
    /// The answer: `None` until there is one, then whether the lock was granted.
    granted: Option<Option<Lock>>,
    waker: Option<Waker>,
    /// The request was dropped, so a lock granted after is released at once.
    dropped: bool,
}

/// A request for a lock, answered by the browser.
struct Request(Rc<RefCell<State>>);

fn request(name: &str, wait: bool) -> Request {
    let state = Rc::new(RefCell::new(State::default()));
    let Some(locks) = web_sys::window()
        .and_then(|window| Reflect::get(&window.navigator(), &"locks".into()).ok())
        .filter(|locks| !locks.is_undefined())
    else {
        state.borrow_mut().granted = Some(Some(Lock { release: None }));
        return Request(state);
    };
    let answered = Rc::clone(&state);
    // called with the lock, or with `null` if it is held and not waited for; the lock is held until
    // the promise returned settles
    let callback = Closure::once_into_js(move |lock: JsValue| -> JsValue {
        let mut state = answered.borrow_mut();
        if lock.is_null() {
            state.granted = Some(None);
        } else if state.dropped {
            return JsValue::UNDEFINED;
        } else {
            let mut release = None;
            let held = Promise::new(&mut |resolve, _| release = Some(resolve));
            state.granted = Some(Some(Lock { release }));
            if let Some(waker) = state.waker.take() {
                waker.wake();
            }
            return held.into();
        }
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
        JsValue::UNDEFINED
    });
    let options = Object::new();
    if !wait {
        Reflect::set(&options, &"ifAvailable".into(), &JsValue::TRUE)
            .expect("an object takes a property");
    }
    // the promise settles when the lock is released; it is rejected only if the callback throws,
    // which it does not
    _ = locks
        .unchecked_into::<LockManager>()
        .request(name, &options, callback.unchecked_ref());
    Request(state)
}

impl Future for Request {
    type Output = Option<Lock>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Lock>> {
        let mut state = self.0.borrow_mut();
        match state.granted.take() {
            Some(granted) => Poll::Ready(granted),
            None => {
                state.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        state.dropped = true;
        // a lock granted but not handed out is released with it
        state.granted = None;
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    use super::*;

    wasm_bindgen_test_configure!(run_in_browser);

    /// A name no other test locks.
    fn unique(name: &str) -> String {
        format!("{name}-{}", uuid::Uuid::new_v4())
    }

    #[wasm_bindgen_test]
    async fn a_lock_is_held_by_one_at_a_time() {
        let name = unique("held");
        let held = try_lock(&name).await;
        assert!(held.is_some());

        assert!(try_lock(&name).await.is_none());
        drop(held);
        assert!(lock_soon(&name).await.is_some());
    }

    #[wasm_bindgen_test]
    async fn a_lock_waited_for_is_taken_once_released() {
        let name = unique("waited");
        let held = try_lock(&name).await.unwrap();
        let mut waiting = Box::pin(lock(&name));
        let mut context = Context::from_waker(Waker::noop());
        assert!(waiting.as_mut().poll(&mut context).is_pending());

        drop(held);

        let taken = waiting.await;
        assert!(try_lock(&name).await.is_none());
        drop(taken);
    }

    #[wasm_bindgen_test]
    async fn giving_up_waiting_leaves_the_lock_to_others() {
        let name = unique("given up");
        let held = try_lock(&name).await.unwrap();
        let mut waiting = Box::pin(lock(&name));
        let mut context = Context::from_waker(Waker::noop());
        assert!(waiting.as_mut().poll(&mut context).is_pending());

        drop(waiting);
        drop(held);

        assert!(lock_soon(&name).await.is_some());
    }

    /// The lock `name` once the browser has handled what was released before, which it does after
    /// a while.
    async fn lock_soon(name: &str) -> Option<Lock> {
        for _ in 0..100 {
            if let Some(lock) = try_lock(name).await {
                return Some(lock);
            }
            let tick = Promise::new(&mut |resolve, _| {
                web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 10)
                    .unwrap();
            });
            wasm_bindgen_futures::JsFuture::from(tick).await.unwrap();
        }
        None
    }
}
