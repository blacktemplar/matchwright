//! The icons of the buttons and links, drawn in the colour of their text. They are hidden from
//! screen readers: the button's or link's text names what it does.

use dioxus::prelude::*;

/// Back, or the one before.
#[component]
pub fn ChevronLeft() -> Element {
    rsx! {
        Icon { path { d: "m15 18-6-6 6-6" } }
    }
}

/// What opens below, or closes again.
#[component]
pub fn ChevronDown() -> Element {
    rsx! {
        Icon { path { d: "m6 9 6 6 6-6" } }
    }
}

/// Moving up, to before the one above.
#[component]
pub fn ArrowUp() -> Element {
    rsx! {
        Icon {
            path { d: "m5 12 7-7 7 7" }
            path { d: "M12 19V5" }
        }
    }
}

/// Moving down, to after the one below.
#[component]
pub fn ArrowDown() -> Element {
    rsx! {
        Icon {
            path { d: "M12 5v14" }
            path { d: "m19 12-7 7-7-7" }
        }
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

/// Renaming.
#[component]
pub fn Pencil() -> Element {
    rsx! {
        Icon {
            path { d: "M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" }
            path { d: "m15 5 4 4" }
        }
    }
}

/// Deleting.
#[component]
pub fn Trash() -> Element {
    rsx! {
        Icon {
            path { d: "M3 6h18" }
            path { d: "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" }
            path { d: "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" }
        }
    }
}

/// A competition won.
#[component]
pub fn Trophy() -> Element {
    rsx! {
        Icon {
            path { d: "M10 14.66v1.626a2 2 0 0 1-.976 1.696A5 5 0 0 0 7 21.978" }
            path { d: "M14 14.66v1.626a2 2 0 0 0 .976 1.696A5 5 0 0 1 17 21.978" }
            path { d: "M18 9h1.5a1 1 0 0 0 0-5H18" }
            path { d: "M4 22h16" }
            path { d: "M6 9a6 6 0 0 0 12 0V3a1 1 0 0 0-1-1H7a1 1 0 0 0-1 1z" }
            path { d: "M6 9H4.5a1 1 0 0 1 0-5H6" }
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

/// The help.
#[component]
pub fn Help() -> Element {
    rsx! {
        Icon {
            circle { cx: "12", cy: "12", r: "10" }
            path { d: "M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" }
            path { d: "M12 17h.01" }
        }
    }
}

/// GitHub, where the source code is.
#[component]
pub fn GitHub() -> Element {
    rsx! {
        Icon {
            path {
                d: concat!(
                    "M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5",
                    ".28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2",
                    "c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5",
                    "-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4",
                ),
            }
            path { d: "M9 18c-4.51 2-5-2-7-2" }
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
