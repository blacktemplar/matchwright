//! Files the operator downloads.

use js_sys::Array;
use wasm_bindgen::{JsCast, JsValue, prelude::Closure};
use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

/// How long the browser is given to read a file being downloaded, in milliseconds.
const READ_FOR: i32 = 60_000;

/// Has the browser download `text` as a file named `name` (which it may change, e.g. to remove
/// characters a file name cannot have) of the media type `media_type`, e.g. `application/json`.
///
/// # Errors
/// Returns what the browser threw if it cannot make the file.
pub fn download(name: &str, media_type: &str, text: &str) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let options = BlobPropertyBag::new();
    options.set_type(media_type);
    let blob = Blob::new_with_str_sequence_and_options(&Array::of1(&text.into()), &options)?;
    let url = Url::create_object_url_with_blob(&blob)?;
    let link: HtmlAnchorElement = document.create_element("a")?.unchecked_into();
    link.set_href(&url);
    link.set_download(name);
    link.click();
    // the download reads the file after the click, so it is only let go of later
    let release = Closure::once_into_js(move || _ = Url::revoke_object_url(&url));
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(release.unchecked_ref(), READ_FOR)?;
    Ok(())
}
