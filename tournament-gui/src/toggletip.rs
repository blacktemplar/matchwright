//! Toggletips: a button next to what it is about, showing a short text about it, so that a list
//! shows no more than names. The text shows while the pointer is on the button or the text, while
//! the button has the focus of the keyboard, and from a click or tap on the button until the next
//! one, a click or tap elsewhere, or the focus leaving it. Escape hides it until the pointer or the
//! focus reaches a toggletip anew. Screen readers say the text as the button's description,
//! whether it shows or not.
//!
//! What shows on hover and focus is the style's (`.toggletip` in `style.css`); the page keeps only
//! which toggletip a click opened, and whether Escape hid the one shown. Each toggletip places its
//! text before it shows: below its button unless the room there is short, extending to the right
//! or to the left, wherever there is more room.

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
    let mut element = use_signal(|| None::<web_sys::Element>);
    let mut placement = use_signal(Placement::default);
    // placed when about to show, since what is around it may have scrolled or changed since
    let mut place = move || {
        if let Some(element) = element.peek().as_ref() {
            let placed = Placement::of(element);
            if *placement.peek() != placed {
                placement.set(placed);
            }
        }
    };
    let Placement { end, above } = placement();
    rsx! {
        span {
            class: "toggletip",
            "data-opened": opened.then_some("true"),
            "data-hidden": hidden.then_some("true"),
            "data-align": end.then_some("end"),
            "data-side": above.then_some("above"),
            onmounted: move |event| {
                element.set(event.data().downcast::<web_sys::Element>().cloned());
            },
            // Escape hides what shows when it is pressed, not one the pointer or the focus reaches
            // after it
            onmouseenter: move |_| {
                place();
                toggletips.unhide();
            },
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
                        place();
                        toggletips.opened.set(Some(opening.clone()));
                        toggletips.unhide();
                    }
                },
                onfocus: move |_| {
                    place();
                    toggletips.unhide();
                },
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

/// Where the text of a toggletip shows, so that it is not cut off: extending towards the middle of
/// where it can show, i.e. of the nearest ancestor that cuts off what reaches out of it (e.g. a
/// dialog that scrolls), or else of the window, and below its button unless the room there is
/// short and there is more above. Below covers what follows the button rather than what comes
/// before it, e.g. the issues of the field before the one whose toggletip the focus reached.
#[derive(Clone, Copy, Default, PartialEq)]
struct Placement {
    /// Extending to the left of the button, which is in the right half.
    end: bool,
    /// Above the button.
    above: bool,
}

/// The room below a button that a text of a toggletip of a few lines takes, in CSS pixels.
const ROOM_BELOW: f64 = 160.0;

impl Placement {
    /// The placement of the text of the toggletip `element`.
    fn of(element: &web_sys::Element) -> Self {
        let window = web_sys::window().expect("the page runs in a window");
        let rect = element.get_bounding_client_rect();
        let x = rect.left() + rect.width() / 2.0;
        // within the bounds `left`, `width`, `top` and `bottom`
        let within = |left: f64, width: f64, top: f64, bottom: f64| {
            let (above, below) = (rect.top() - top, bottom - rect.bottom());
            Self {
                end: x > left + width / 2.0,
                above: below < ROOM_BELOW && above > below,
            }
        };
        let mut ancestor = element.parent_element();
        while let Some(current) = ancestor {
            let cuts_off = window
                .get_computed_style(&current)
                .ok()
                .flatten()
                .and_then(|style| style.get_property_value("overflow-x").ok())
                .is_some_and(|overflow| overflow != "visible");
            if cuts_off {
                let bounds = current.get_bounding_client_rect();
                return within(bounds.left(), bounds.width(), bounds.top(), bounds.bottom());
            }
            ancestor = current.parent_element();
        }
        let size = |size: Result<wasm_bindgen::JsValue, _>| {
            size.ok().and_then(|size| size.as_f64()).unwrap_or_default()
        };
        within(
            0.0,
            size(window.inner_width()),
            0.0,
            size(window.inner_height()),
        )
    }
}
