//! The colours of the page: light or dark as the system prefers, or as the operator chose, which
//! the browser keeps.
//!
//! The choice is the attribute `data-theme` of `<html>`, which the style follows (absent for the
//! system's). `index.html` sets it from local storage before the page is drawn, so that it does
//! not flash in the other colours first.

/// What decides whether the page is light or dark.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// What the system prefers.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// How local storage and `data-theme` name it; `index.html` reads the same names.
    pub fn tag(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.tag() == tag)
    }

    /// The key of the message naming it.
    pub fn message(self) -> &'static str {
        match self {
            Self::System => "theme-system",
            Self::Light => "theme-light",
            Self::Dark => "theme-dark",
        }
    }
}

/// Where the browser's local storage keeps the theme chosen; `index.html` reads it too.
const CHOSEN: &str = "tournament-gui.theme";

/// The theme chosen last in this browser, or else the system's.
pub fn initial() -> Theme {
    local_storage()
        .and_then(|storage| storage.get_item(CHOSEN).ok().flatten())
        .and_then(|tag| Theme::from_tag(&tag))
        .unwrap_or_default()
}

/// Keeps `theme` as the one chosen in this browser.
pub fn remember(theme: Theme) {
    // without local storage, the choice lasts until the page is left
    if let Some(storage) = local_storage() {
        _ = storage.set_item(CHOSEN, theme.tag());
    }
}

/// Shows the page in the colours of `theme`.
pub fn apply(theme: Theme) {
    let Some(html) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    // fail only for an invalid attribute name
    _ = match theme {
        Theme::System => html.remove_attribute("data-theme"),
        Theme::Light | Theme::Dark => html.set_attribute("data-theme", theme.tag()),
    };
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}
