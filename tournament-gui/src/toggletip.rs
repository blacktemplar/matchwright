//! Toggletips: a button next to what it is about, showing a short text about it, so that a list
//! shows no more than names. The text shows while the pointer is on the button or the text, while
//! the button has the focus of the keyboard, and from a click or tap on the button until the next
//! one, a click or tap elsewhere, or the focus leaving it. Escape hides it until the pointer or the
//! focus reaches a toggletip anew. Screen readers say the text as the button's description,
//! whether it shows or not.
//!
//! What shows on hover and focus is the style's (`.toggletip` in `style.css`); the page keeps only
//! which toggletip a click opened, and whether Escape hid the one shown.

use dioxus::prelude::*;
use futures_util::StreamExt as _;
use tournament_browser::channel::Events;
use wasm_bindgen::JsCast as _;
use web_sys::KeyboardEvent;

use crate::{Page, i18n::t, icons};

/// What the toggletips of the page share.
#[derive(Clone, Copy)]
struct Toggletips {
    /// The id of the text of the toggletip a click opened.
    opened: Signal<Option<String>>,
    /// Whether Escape hid the toggletip shown.
    hidden: Signal<bool>,
}

/// Provides what the toggletips of the page share, and closes them on Escape anywhere and the one a
/// click opened on a click or tap outside any toggletip. Called once, by the page.
pub fn use_toggletips() {
    let mut toggletips = use_context_provider(|| Toggletips {
        opened: Signal::new(None),
        hidden: Signal::new(false),
    });
    use_future(move || async move {
        let window = web_sys::window().expect("the page runs in a window");
        let mut keys = Events::new(&window, "keydown");
        while let Some(event) = keys.next().await {
            let escape = event
                .dyn_ref::<KeyboardEvent>()
                .is_some_and(|event| event.key() == "Escape");
            if escape {
                toggletips.close();
                toggletips.hidden.set(true);
            }
        }
    });
    use_future(move || async move {
        let window = web_sys::window().expect("the page runs in a window");
        // a pointer pressed rather than a click, which Safari does not send for a tap on text
        let mut presses = Events::new(&window, "pointerdown");
        while let Some(event) = presses.next().await {
            let on_toggletip = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .and_then(|element| element.closest(".toggletip").ok().flatten())
                .is_some();
            if !on_toggletip {
                toggletips.close();
            }
        }
    });
}

impl Toggletips {
    /// Closes the toggletip a click opened, if any.
    fn close(mut self) {
        if self.opened.peek().is_some() {
            self.opened.set(None);
        }
    }

    /// Shows the toggletips on hover and focus again after Escape hid them.
    fn unhide(mut self) {
        if *self.hidden.peek() {
            self.hidden.set(false);
        }
    }
}

/// A toggletip saying `text` about `subject`, e.g. the label of a plugin, which names its button
/// for screen readers. `id` is that of the element of the text, unique on the page, which an
/// element may name as its description.
#[component]
pub fn Toggletip(id: String, subject: String, text: String) -> Element {
    let page = use_context::<Page>();
    let mut toggletips = use_context::<Toggletips>();
    let tr = page.translator.read();
    let opened = toggletips.opened.read().as_deref() == Some(id.as_str());
    let hidden = *toggletips.hidden.read();
    let (opening, closing) = (id.clone(), id.clone());
    rsx! {
        span {
            class: "toggletip",
            "data-opened": opened.then_some("true"),
            "data-hidden": hidden.then_some("true"),
            // Escape hides what shows when it is pressed, not one the pointer or the focus reaches
            // after it
            onmouseenter: move |_| toggletips.unhide(),
            onmouseleave: move |_| toggletips.unhide(),
            button {
                r#type: "button",
                class: "quiet small",
                aria_label: t!(tr, "about", subject = subject.as_str()),
                aria_describedby: "{id}",
                onclick: move |_| {
                    if opened {
                        // hidden as well, since the pointer or the focus on it would keep it shown
                        toggletips.close();
                        toggletips.hidden.set(true);
                    } else {
                        toggletips.opened.set(Some(opening.clone()));
                        toggletips.unhide();
                    }
                },
                onfocus: move |_| toggletips.unhide(),
                onblur: move |_| {
                    if toggletips.opened.peek().as_deref() == Some(closing.as_str()) {
                        toggletips.close();
                    }
                    toggletips.unhide();
                },
                icons::Info {}
            }
            span { class: "toggletip-text", id: "{id}", role: "tooltip", "{text}" }
        }
    }
}
