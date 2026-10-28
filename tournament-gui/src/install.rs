//! Installing the page as an app, which Chromium-based browsers offer with the event
//! `beforeinstallprompt`. The offer may come before the WebAssembly runs, so `public/pwa.js` keeps
//! it as `window.matchwrightInstallPrompt` and announces it with the event [`OFFERED`].

use js_sys::{Function, Reflect};
use wasm_bindgen::{JsCast, JsValue};

/// The event of the window that announces an offer.
pub const OFFERED: &str = "matchwright-installable";

/// The event of the window that tells that the page was installed.
pub const INSTALLED: &str = "appinstalled";

/// Where `pwa.js` keeps the offer.
const KEPT: &str = "matchwrightInstallPrompt";

/// Whether the browser offers to install the page now.
pub fn offered() -> bool {
    kept().is_some()
}

/// Shows the browser's dialog that installs the page, if it offers to; [`INSTALLED`] tells if the
/// operator installed it. An offer is shown once, so the page is offered again only if the browser
/// announces another one.
pub fn install() {
    let Some((window, offer)) = kept() else {
        return;
    };
    _ = Reflect::delete_property(&window, &KEPT.into());
    if let Some(prompt) = property(&offer, "prompt").and_then(|prompt| prompt.dyn_into().ok()) {
        let prompt: Function = prompt;
        _ = prompt.call0(&offer);
    }
}

/// The window and the offer `pwa.js` keeps there, if any.
fn kept() -> Option<(web_sys::Window, JsValue)> {
    let window = web_sys::window()?;
    let offer = property(&window, KEPT)?;
    Some((window, offer))
}

/// The property `name` of `object`, unless it has none.
fn property(object: &JsValue, name: &str) -> Option<JsValue> {
    Reflect::get(object, &name.into())
        .ok()
        .filter(|value| !value.is_undefined() && !value.is_null())
}
