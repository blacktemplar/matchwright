//! Whether the browser keeps what the page stores. Without being asked to, a browser short of space
//! may delete it.

use wasm_bindgen_futures::JsFuture;

/// Asks the browser to keep what the page stores even when it runs short of space, unless it does
/// already, and returns whether it does. Some browsers ask the operator, others decide by how much
/// the page is used; so ask when the operator does something with the page, e.g. creates a
/// competition.
pub async fn persist() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    // a page served over plain HTTP from elsewhere than `localhost` has no storage manager, and
    // calling it fails
    let storage = window.navigator().storage();
    let answer = async |promise: Result<js_sys::Promise, _>| match promise {
        Ok(promise) => JsFuture::from(promise)
            .await
            .is_ok_and(|answer| answer.as_bool() == Some(true)),
        Err(_) => false,
    };
    answer(storage.persisted()).await || answer(storage.persist()).await
}
