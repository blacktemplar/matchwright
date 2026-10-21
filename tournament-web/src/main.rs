//! The page of `index.html`: a command line running a [`Shell`].
//!
//! The part of the URL after `#` names the competition, like the file argument of the CLI: the page
//! opens the competition saved under it, and sets it to the name the competition is saved under.

use std::{cell::RefCell, rc::Rc};

use tournament_web::{Reply, Shell};
use wasm_bindgen::{JsCast, JsValue, prelude::Closure};
use web_sys::{Document, Element, Event, HtmlInputElement, KeyboardEvent, Window};

fn main() {
    console_error_panic_hook::set_once();
    let window = web_sys::window().expect("the page runs in a window");
    let document = window.document().expect("a window has a document");
    let element = |id: &str| {
        document
            .get_element_by_id(id)
            .unwrap_or_else(|| panic!("index.html must have an element #{id}"))
    };
    let input: HtmlInputElement = element("input")
        .dyn_into()
        .expect("#input must be an input");
    let page = Page {
        shell: Shell::new(),
        output: element("output"),
        prompt: element("prompt"),
        history: Vec::new(),
        cursor: 0,
        window: window.clone(),
        document: document.clone(),
    };
    let page = Rc::new(RefCell::new(page));
    page.borrow_mut().start();

    let on_key = {
        let page = Rc::clone(&page);
        let input = input.clone();
        Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
            let mut page = page.borrow_mut();
            match event.key().as_str() {
                "Enter" => {
                    let line = input.value();
                    input.set_value("");
                    page.run(&line);
                }
                "ArrowUp" => {
                    event.prevent_default();
                    input.set_value(&page.recall(-1));
                }
                "ArrowDown" => {
                    event.prevent_default();
                    input.set_value(&page.recall(1));
                }
                _ => {}
            }
        })
    };
    input
        .add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref())
        .expect("an input takes key listeners");
    // the listener lives as long as the page
    on_key.forget();

    let on_unload = {
        let page = Rc::clone(&page);
        Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            // asks whether to leave the page and discard the unsaved changes
            if page.borrow().shell.has_unsaved_changes() {
                event.prevent_default();
            }
        })
    };
    window
        .add_event_listener_with_callback("beforeunload", on_unload.as_ref().unchecked_ref())
        .expect("a window takes unload listeners");
    on_unload.forget();
}

/// The state of the page, which its event listeners share.
struct Page {
    shell: Shell,
    /// Where the lines run and what they show are listed.
    output: Element,
    /// Where the prompt of the shell is shown, in front of the input.
    prompt: Element,
    /// The lines run so far, oldest first.
    history: Vec<String>,
    /// The line of the history the input shows, or the length of the history for a new line.
    cursor: usize,
    window: Window,
    document: Document,
}

impl Page {
    /// Opens the competition the URL names, if any.
    fn start(&mut self) {
        if let Some(name) = self.named_in_url() {
            match Shell::open(&name) {
                Ok(shell) => self.shell = shell,
                Err(error) => self.show("error", &format!("error: {error}")),
            }
        }
        self.show("output", "type `help` for the commands");
        self.update();
    }

    /// Runs `line`, showing it after the prompt and then what it shows.
    fn run(&mut self, line: &str) {
        self.show("line", &format!("{}{line}", self.shell.prompt()));
        if !line.trim().is_empty() {
            self.history.push(line.to_owned());
        }
        self.cursor = self.history.len();
        for reply in self.shell.run(line) {
            match reply {
                Reply::Output(output) => self.show("output", &output),
                Reply::Error(error) => self.show("error", &error),
            }
        }
        self.update();
    }

    /// Moves `step` lines through the history, returning the line it arrives at; past the newest
    /// line it arrives at an empty one.
    fn recall(&mut self, step: isize) -> String {
        self.cursor = self
            .cursor
            .saturating_add_signed(step)
            .min(self.history.len());
        self.history.get(self.cursor).cloned().unwrap_or_default()
    }

    /// Appends `text` to the output, styled as `class`, and scrolls it into view.
    fn show(&self, class: &str, text: &str) {
        let entry = self
            .document
            .create_element("div")
            .expect("a document creates divs");
        entry.set_class_name(class);
        entry.set_text_content(Some(text));
        self.output
            .append_child(&entry)
            .expect("an element takes children");
        entry.scroll_into_view();
    }

    /// Shows the prompt of the shell, and names its competition in the URL.
    fn update(&self) {
        self.prompt.set_text_content(Some(&self.shell.prompt()));
        if self.named_in_url().as_deref() == self.shell.name() {
            return;
        }
        let url = match self.shell.name() {
            Some(name) => format!("#{}", js_sys::encode_uri_component(name)),
            None => self.window.location().pathname().unwrap_or_default(),
        };
        // replacing the URL rather than adding one keeps the back button leaving the page
        self.window
            .history()
            .and_then(|history| history.replace_state_with_url(&JsValue::NULL, "", Some(&url)))
            .expect("the page can change its URL");
    }

    /// The name after the `#` of the URL, if there is one.
    fn named_in_url(&self) -> Option<String> {
        let hash = self.window.location().hash().ok()?;
        let name = js_sys::decode_uri_component(hash.strip_prefix('#')?).ok()?;
        Some(String::from(name)).filter(|name| !name.is_empty())
    }
}
