//! The icons of the buttons, drawn in the colour of their text. They are hidden from screen
//! readers: the button's text names what it does.

use dioxus::prelude::*;

/// Back, or the one before.
#[component]
pub fn ChevronLeft() -> Element {
    rsx! {
        Icon { path { d: "m15 18-6-6 6-6" } }
    }
}

/// The one after.
#[component]
pub fn ChevronRight() -> Element {
    rsx! {
        Icon { path { d: "m9 18 6-6-6-6" } }
    }
}

#[component]
pub fn Undo() -> Element {
    rsx! {
        Icon {
            path { d: "M9 14 4 9l5-5" }
            path { d: "M4 9h10.5a5.5 5.5 0 0 1 0 11H11" }
        }
    }
}

#[component]
pub fn Redo() -> Element {
    rsx! {
        Icon {
            path { d: "m15 14 5-5-5-5" }
            path { d: "M20 9H9.5a5.5 5.5 0 0 0 0 11H13" }
        }
    }
}

#[component]
pub fn Export() -> Element {
    rsx! {
        Icon {
            path { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }
            path { d: "m7 10 5 5 5-5" }
            path { d: "M12 15V3" }
        }
    }
}

/// What is said about something.
#[component]
pub fn Info() -> Element {
    rsx! {
        Icon {
            circle { cx: "12", cy: "12", r: "10" }
            path { d: "M12 16v-4" }
            path { d: "M12 8h.01" }
        }
    }
}

/// A square icon of 24 units drawn by the strokes of `children`.
#[component]
fn Icon(children: Element) -> Element {
    rsx! {
        svg {
            class: "icon",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            {children}
        }
    }
}
