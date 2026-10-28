//! What the GUI says, in the operator's language: its messages, [`t!`] to say one, and the
//! language chosen, which the browser keeps.

use tournament_app::i18n::Language;

/// The messages of the GUI in `language`, see [`Resources`](tournament_app::i18n::Resources).
pub fn resources(language: Language) -> &'static str {
    match language {
        Language::English => include_str!("../locales/en/gui.ftl"),
        Language::German => include_str!("../locales/de/gui.ftl"),
    }
}

/// The text of the message `key` in the language of `translator`, a
/// [`Translator`](tournament_app::i18n::Translator), with its variables given as `name = value`:
/// `t!(translator, "round", round = 2)`.
macro_rules! t {
    ($translator:expr, $key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $translator.text(
            &::tournament_app::i18n::Text::new($key) $(.with(stringify!($name), $value))*
        )
    };
}
pub(crate) use t;

/// Where the browser's local storage keeps the language chosen.
const CHOSEN: &str = "matchwright.language";

/// The language chosen last in this browser, or else the first the browser prefers that the GUI
/// speaks, or else English.
pub fn initial_language() -> Language {
    let Some(window) = web_sys::window() else {
        return Language::default();
    };
    let chosen = window
        .local_storage()
        .ok()
        .flatten()
        .and_then(|storage| storage.get_item(CHOSEN).ok().flatten())
        .and_then(|tag| Language::from_tag(&tag));
    chosen.unwrap_or_else(|| {
        let preferred: Vec<String> = window
            .navigator()
            .languages()
            .iter()
            .filter_map(|tag| tag.as_string())
            .collect();
        Language::negotiate(preferred.iter().map(String::as_str))
    })
}

/// Keeps `language` as the one chosen in this browser.
pub fn remember(language: Language) {
    // without local storage, e.g. when the browser forbids it, the choice lasts until the page is
    // left
    if let Some(storage) =
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        _ = storage.set_item(CHOSEN, language.tag());
    }
}

/// Makes `language` the language of the page, its `<html lang>`.
pub fn set_page_language(language: Language) {
    if let Some(html) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    {
        // fails only for an invalid attribute name
        _ = html.set_attribute("lang", language.tag());
    }
}

#[cfg(test)]
mod tests {
    use tournament_app::i18n::check;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    #[test]
    fn the_messages_of_the_gui_are_in_every_language() {
        check(resources).unwrap();
    }
}
