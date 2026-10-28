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

/// A medal on its ribbon, the disc filled by the CSS of `class`, e.g. in gold.
#[component]
pub fn Medal(class: &'static str) -> Element {
    rsx! {
        span { class: "medal {class}",
            Icon {
                path {
                    d: "M7.21 15 2.66 7.14a2 2 0 0 1 .13-2.2L4.4 2.8A2 2 0 0 1 6 2h12 \
                        a2 2 0 0 1 1.6.8l1.6 2.14a2 2 0 0 1 .14 2.2L16.79 15",
                }
                path { d: "M11 12 5.12 2.2" }
                path { d: "m13 12 5.88-9.8" }
                path { d: "M8 7h8" }
                circle { cx: "12", cy: "17", r: "5" }
            }
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
