//! The phase: the form configuring it, generated from the [`FormView`] of a [`PhaseForm`], a
//! summary of how it is played, and the dialog changing it once added.

use dioxus::prelude::*;
use tournament_app::{
    form::{
        FormField, FormView, Input, Item, ListInput, Path, PhaseForm, PluginInput, PluginOption,
        SetEntry,
    },
    i18n::{Message as _, PluginText, Text, Translator},
};

use crate::{
    capitalized,
    competition::Ctx,
    components::{
        dialog::{Dialog, DialogDescription, DialogTitle},
        select::{Select, SelectOption},
    },
    i18n::t,
    icons,
    toggletip::Toggletip,
};

/// The form of a new phase, and the button adding it.
#[component]
pub fn NewPhase() -> Element {
    let ctx = use_context::<Ctx>();
    let created = use_hook(|| {
        ctx.session
            .peek()
            .phase_form()
            .map(Signal::new)
            .map_err(|error| error.text())
    });
    let actions = ctx.actions();
    let tr = ctx.translator.read();
    let form = match created {
        Ok(form) => form,
        Err(error) => return rsx! { p { class: "error", {tr.sentence(&error)} } },
    };
    rsx! {
        form {
            id: "new-phase",
            onsubmit: move |event| {
                event.prevent_default();
                if !form.read().view().complete {
                    focus_first_issue("new-phase");
                    return;
                }
                if ctx.act(|session| session.add_phase(form.read().draft())).is_some() {
                    // the form is gone, and with it what had the focus
                    focus_later(&["#start:enabled", "#phase"]);
                }
            },
            Form { form, disabled: !actions.add_phase }
            div { class: "form-actions",
                button {
                    class: "primary",
                    r#type: "submit",
                    disabled: !actions.add_phase,
                    {tr.get("add-phase")}
                }
            }
        }
    }
}

/// How the phase is played, and a button opening the dialog that changes it.
#[component]
pub fn PhaseSummary() -> Element {
    let ctx = use_context::<Ctx>();
    let Ok(form) = ctx.session.read().phase_form_to_edit() else {
        return rsx! {};
    };
    let actions = ctx.actions();
    rsx! {
        Summary { fields: form.view().fields }
        if actions.edit_phase {
            EditPhase {}
        }
    }
}

/// The button editing the phase, and the dialog it opens: the form of the phase as it is played.
#[component]
fn EditPhase() -> Element {
    let ctx = use_context::<Ctx>();
    // the form while the dialog is open
    let mut editing = use_signal(|| None::<Signal<PhaseForm>>);
    let mut error = use_signal(|| None::<Text>);
    let tr = ctx.translator.read();
    let open = editing.read().is_some();
    rsx! {
        button {
            onclick: move |_| {
                if let Ok(form) = ctx.session.peek().phase_form_to_edit() {
                    error.set(None);
                    editing.set(Some(Signal::new(form)));
                }
            },
            {tr.get("edit-phase")}
        }
        Dialog {
            open: Some(open),
            on_open_change: move |open: bool| {
                if !open {
                    editing.set(None);
                }
            },
            DialogTitle { {tr.get("edit-phase-title")} }
            DialogDescription { {tr.get("edit-phase-description")} }
            if let Some(form) = editing() {
                form {
                    id: "edit-phase",
                    class: "dialog-form",
                    onsubmit: move |event| {
                        event.prevent_default();
                        if !form.read().view().complete {
                            error.set(None);
                            focus_first_issue("edit-phase");
                            return;
                        }
                        match ctx.try_act(|session| session.edit_phase(form.read().draft())) {
                            Ok(_) => editing.set(None),
                            Err(refused) => error.set(Some(refused)),
                        }
                    },
                    Form { form, disabled: false }
                    p { class: "error", role: "alert",
                        if let Some(error) = error() {
                            {tr.sentence(&error)}
                        }
                    }
                    div { class: "dialog-buttons",
                        button { class: "primary", r#type: "submit", {tr.get("save-changes")} }
                        button { r#type: "button", onclick: move |_| editing.set(None),
                            {tr.get("cancel")}
                        }
                    }
                }
            }
        }
    }
}

/// The fields of `form`, which the operator changes unless `disabled`, with what is wrong with
/// the answers as a whole.
#[component]
fn Form(form: Signal<PhaseForm>, disabled: bool) -> Element {
    let FormView { fields, issues, .. } = form.read().view();
    rsx! {
        div { class: "phase-form",
            Fields { form, fields, disabled }
            Issues { id: "phase-issues", issues }
        }
    }
}

#[component]
fn Fields(form: Signal<PhaseForm>, fields: Vec<FormField>, disabled: bool) -> Element {
    rsx! {
        for field in fields {
            Field { key: "{field.path}", form, field, disabled }
        }
    }
}

/// One field, laid out by the kind of its answer.
#[component]
fn Field(form: Signal<PhaseForm>, field: FormField, disabled: bool) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let id = id(&field.path);
    let label = capitalized(&tr.field_label(&field));
    let description = tr.field_description(&field);
    let path = field.path.clone();
    let described_by = format!("{id}-description {id}-issues {id}-warnings");
    let issues = rsx! {
        Issues { id: "{id}-issues", issues: field.issues.clone() }
        Warnings { id: "{id}-warnings", warnings: field.warnings.clone() }
    };
    // what is said about the field, next to its label
    let about = rsx! {
        if let Some(text) = description {
            " "
            Toggletip { id: "{id}-description", subject: tr.field_label(&field), text }
        }
    };
    match field.input.clone() {
        Input::Bool(value) => rsx! {
            div { class: "field",
                label {
                    input {
                        r#type: "checkbox",
                        id: "{id}",
                        checked: value.unwrap_or_default(),
                        disabled,
                        aria_describedby: "{described_by}",
                        onchange: move |event| _ = form.write().set_bool(&path, event.checked()),
                    }
                    " {label}"
                }
                {about}
                {issues}
            }
        },
        Input::Integer {
            range,
            value,
            entered,
            hint,
        } => {
            let shown = entered.or_else(|| value.map(|value| value.to_string()));
            rsx! {
                NumberField {
                    form, path, id, label, disabled,
                    value: shown,
                    // a minus as well, where the keyboard of the numbers has none
                    keyboard: if range.min() < 0 { "text" } else { "numeric" },
                    hint: tr.text(&hint),
                    about,
                    {issues}
                }
            }
        }
        Input::Decimal {
            range,
            value,
            entered,
            hint,
        } => {
            let shown = entered.or_else(|| value.map(|value| tr.number(value)));
            let negative = range.min().is_none_or(|min| min < 0.0);
            rsx! {
                NumberField {
                    form, path, id, label, disabled,
                    value: shown,
                    keyboard: if negative { "text" } else { "decimal" },
                    hint: tr.text(&hint),
                    about,
                    {issues}
                }
            }
        }
        Input::Optional { answered, inner } => {
            // described once, here
            let inner = FormField {
                input: *inner,
                description: None,
                issues: Vec::new(),
                warnings: Vec::new(),
                ..field.clone()
            };
            rsx! {
                div { class: "field",
                    label {
                        input {
                            r#type: "checkbox",
                            checked: answered,
                            disabled,
                            onchange: move |event| {
                                _ = form.write().answer_optional(&path, event.checked());
                            },
                        }
                        " {label}"
                    }
                    {about}
                    if answered {
                        Field { form, field: inner, disabled }
                    }
                    {issues}
                }
            }
        }
        Input::Plugin(plugin) => rsx! {
            PluginField { form, path, id, label, plugin, disabled, about,
                {issues}
            }
        },
        // a group rather than a fieldset, whose legend would take the toggletip's button into its
        // name
        Input::List(list) => rsx! {
            div {
                class: "field list",
                role: "group",
                aria_labelledby: "{id}-label",
                aria_describedby: "{described_by}",
                div { class: "list-heading",
                    span { class: "label", id: "{id}-label", "{label}" }
                    {about}
                }
                if list.ordered {
                    OrderedList { form, path, id: id.clone(), list, disabled }
                } else {
                    UnorderedList { form, path, list_id: id.clone(), list, disabled }
                }
                {issues}
            }
        },
    }
}

/// A field taking a number, entered as text and taken once the operator leaves the field, with
/// what is said about it, `about`, next to its label. `keyboard` is the `inputmode` of the input,
/// the keyboard a touch screen shows.
///
/// The input is a text: one of the type `number` hands over nothing for what it does not read as
/// a number, e.g. `0,5` where the browser expects a point, so that the form could not say what was
/// entered, and it would read it otherwise than the form.
#[component]
fn NumberField(
    form: Signal<PhaseForm>,
    path: Path,
    id: String,
    label: String,
    value: Option<String>,
    keyboard: String,
    hint: String,
    disabled: bool,
    about: Element,
    children: Element,
) -> Element {
    // what is typed, if anything, taken on Enter as well, which does not submit the form
    let mut typed = use_signal(|| None::<String>);
    let take = path.clone();
    rsx! {
        // on a grid of a column of labels and one of inputs, the numbers it takes below its input
        div { class: "field number-field",
            span { class: "number-label",
                label { r#for: "{id}", "{label}" }
                {about}
            }
            input {
                r#type: "text",
                class: "number",
                id: "{id}",
                inputmode: keyboard,
                autocomplete: "off",
                value: value.unwrap_or_default(),
                disabled,
                aria_describedby: "{id}-hint {id}-description {id}-issues",
                oninput: move |event| typed.set(Some(event.value())),
                onkeydown: move |event| {
                    if event.key() == Key::Enter {
                        event.prevent_default();
                        if let Some(typed) = typed.take() {
                            _ = form.write().enter(&take, &typed);
                        }
                    }
                },
                onchange: move |event| _ = form.write().enter(&path, &event.value()),
            }
            span { class: "hint", id: "{id}-hint", {capitalized(&hint)} }
            {children}
        }
    }
}

/// A field taking one plugin, chosen from its options, followed by the fields of the plugin, with
/// what is said about it, `about`, next to its label.
#[component]
fn PluginField(
    form: Signal<PhaseForm>,
    path: Path,
    id: String,
    label: String,
    plugin: PluginInput,
    disabled: bool,
    about: Element,
    children: Element,
) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let chosen = plugin
        .chosen
        .as_ref()
        .map(|chosen| chosen.plugin.id.to_owned());
    let none = plugin.optional.then(|| tr.get("no-plugin"));
    let choose = path.clone();
    let trigger = format!("#{id}");
    // what the plugin chosen does, below the choice, rather than a second ⓘ next to the field's
    let chosen_description = plugin
        .chosen
        .as_ref()
        .and_then(|chosen| plugin_description(&tr, &chosen.plugin));
    rsx! {
        div { class: "field",
            span { class: "label", id: "{id}-label", "{label}" }
            {about}
            " "
            Select {
                id: "{id}",
                labelled_by: "{id}-label",
                described_by: chosen_description.is_some().then(|| format!("{id}-chosen")),
                value: chosen.or_else(|| plugin.optional.then(String::new)),
                placeholder: tr.get("choose-plugin"),
                disabled,
                // the option of no plugin has no id
                on_change: move |chosen: String| {
                    let chosen = Some(chosen.as_str()).filter(|chosen| !chosen.is_empty());
                    _ = form.write().choose(&choose, chosen);
                    // back from the list, which is hidden once an option is chosen
                    focus_later(&[&trigger]);
                },
                for (index, option) in plugin.options.iter().enumerate() {
                    PluginChoice {
                        key: "{option.id}",
                        select: id.clone(),
                        index,
                        option: *option,
                    }
                }
                if let Some(none) = none {
                    SelectOption {
                        index: plugin.options.len(),
                        value: String::new(),
                        text_value: none.clone(),
                        span { class: "option-label", "{none}" }
                    }
                }
            }
            if let Some(description) = chosen_description {
                p { class: "hint chosen-description", id: "{id}-chosen",
                    {capitalized(&description)}
                }
            }
            {children}
            if let Some(chosen) = plugin.chosen {
                div { class: "plugin-fields",
                    Fields { form, fields: chosen.fields, disabled }
                }
            }
        }
    }
}

/// `option` as an option of the [`Select`] whose id is `select`, at `index`. Its description
/// shows next to the list while the option is under the pointer or has the focus, which the
/// keyboard moves through the options, and in the option, below its label, from a tap or click
/// on its ⓘ until the next, which is how a touch screen shows it before the option is chosen.
#[component]
fn PluginChoice(select: String, index: usize, option: PluginOption) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let label = capitalized(&plugin_label(&tr, &option));
    let description = plugin_description(&tr, &option);
    let mut tapped = use_signal(|| false);
    // ids of plugins are namespaced by `::`
    let described_by = format!("{select}-option-{}", option.id.replace("::", "-"));
    rsx! {
        SelectOption {
            index,
            value: option.id.to_owned(),
            text_value: label.clone(),
            aria_describedby: description.is_some().then(|| described_by.clone()),
            span { class: "option",
                span { class: "option-label", "{label}" }
                if option.recommended {
                    " "
                    span { class: "recommended", {tr.get("recommended")} }
                }
                if let Some(description) = description {
                    // no button: a keyboard and a screen reader get the description through the
                    // option, and a button would be an element to use within one; the press is
                    // kept from the option, which would choose it on its release
                    span {
                        class: "option-info",
                        "aria-hidden": "true",
                        onpointerdown: move |event| {
                            event.stop_propagation();
                            event.prevent_default();
                        },
                        onclick: move |event| {
                            event.stop_propagation();
                            tapped.toggle();
                        },
                        icons::Info {}
                    }
                    span {
                        class: "option-description",
                        id: "{described_by}",
                        "data-tapped": tapped().then_some("true"),
                        span { class: "option-description-text", "{description}" }
                    }
                }
            }
        }
    }
}

/// A list of plugins whose order matters: each can be moved, removed and configured, and one more
/// added at the end.
#[component]
fn OrderedList(
    form: Signal<PhaseForm>,
    path: Path,
    id: String,
    list: ListInput,
    disabled: bool,
) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let last = list.items.len().saturating_sub(1);
    let implied = implied(&tr, &list.implied);
    // the plugin added, at the end, or the list adding another one
    let added = format!(
        "#{}-remove",
        self::id(&path.clone().index(list.items.len()))
    );
    let add_trigger = format!("#{id}-add");
    rsx! {
        ol { class: "plugin-list", id: "{id}",
            // keyed by the plugin, so that one moved keeps its element
            for (index, item) in list.items.into_iter().enumerate() {
                OrderedItem {
                    key: "{item.plugin.id}",
                    list: id.clone(),
                    form,
                    list_path: path.clone(),
                    index,
                    item,
                    last: index == last,
                    disabled,
                }
            }
            for implied in implied {
                li { key: "{implied.option.id}", class: "implied",
                    "{implied.text} "
                    About { list: id.clone(), option: implied.option }
                }
            }
        }
        if !list.options.is_empty() {
            div { class: "add",
                span { class: "label", id: "{id}-add-label", {tr.get("add-plugin")} }
                " "
                Select {
                    id: "{id}-add",
                    labelled_by: "{id}-add-label",
                    value: None,
                    placeholder: tr.get("choose-plugin"),
                    disabled,
                    on_change: move |chosen: String| {
                        _ = form.write().add(&path, &chosen);
                        focus_later(&[&add_trigger, &added]);
                    },
                    for (index, option) in list.options.iter().enumerate() {
                        PluginChoice {
                            key: "{option.id}",
                            select: "{id}-add",
                            index,
                            option: *option,
                        }
                    }
                }
            }
        }
    }
}

/// A plugin of a list whose order matters, at `index` of the list at `list_path`, with the buttons
/// moving and removing it, and its fields.
#[component]
fn OrderedItem(
    form: Signal<PhaseForm>,
    list: String,
    list_path: Path,
    index: usize,
    item: Item,
    last: bool,
    disabled: bool,
) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let name = plugin_label(&tr, &item.plugin);
    let (up, down, remove) = (item.path.clone(), item.path.clone(), item.path.clone());
    // the button of the item at `index`, by its position
    let button = move |index: usize, button: &str| {
        format!("#{}-{button}", id(&list_path.clone().index(index)))
    };
    // moving it moves its element, which loses the focus, to the position it is moved to
    let (moved_up, moved_down) = (
        button(index.saturating_sub(1), "up"),
        button(index + 1, "down"),
    );
    // after the removal the next item is at the position of the one removed, else the one before
    // it is the last, else the list adding one is all there is
    let mut after_removal = vec![button(index, "remove")];
    if let Some(before) = index.checked_sub(1) {
        after_removal.push(button(before, "remove"));
    }
    after_removal.push(format!("#{list}-add"));
    let own = id(&item.path);
    rsx! {
        li {
            span { class: "name", {capitalized(&name)} }
            " "
            About { list, option: item.plugin }
            span { class: "item-buttons",
                // kept focusable at the end of the list, so that the focus stays on a plugin
                // moved there
                button {
                    class: "small icon-only",
                    r#type: "button",
                    id: "{own}-up",
                    aria_label: t!(tr, "move-up", plugin = name.as_str()),
                    disabled,
                    aria_disabled: (index == 0).then_some("true"),
                    onclick: move |_| {
                        _ = form.write().move_up(&up);
                        focus_later(&[&moved_up]);
                    },
                    icons::ArrowUp {}
                }
                button {
                    class: "small icon-only",
                    r#type: "button",
                    id: "{own}-down",
                    aria_label: t!(tr, "move-down", plugin = name.as_str()),
                    disabled,
                    aria_disabled: last.then_some("true"),
                    onclick: move |_| {
                        _ = form.write().move_down(&down);
                        // nothing at the end, where it stays
                        focus_later(&[&moved_down]);
                    },
                    icons::ArrowDown {}
                }
                button {
                    class: "small",
                    r#type: "button",
                    id: "{own}-remove",
                    aria_label: t!(tr, "remove-plugin", plugin = name.as_str()),
                    disabled,
                    onclick: move |_| {
                        _ = form.write().remove(&remove);
                        let selectors: Vec<&str> =
                            after_removal.iter().map(String::as_str).collect();
                        focus_later(&selectors);
                    },
                    {tr.get("remove")}
                }
            }
            Fields { form, fields: item.fields, disabled }
            Issues { id: "{id(&item.path)}-issues", issues: item.issues }
        }
    }
}

/// A plugin added to a list since another one needs it.
struct Implied {
    option: PluginOption,
    /// What the list shows for it.
    text: String,
}

/// What a list shows for the plugins of `implied`.
fn implied(tr: &Translator, implied: &[PluginOption]) -> Vec<Implied> {
    implied
        .iter()
        .map(|option| {
            let plugin = plugin_label(tr, option);
            Implied {
                option: *option,
                text: capitalized(&t!(tr, "implied-plugin", plugin = plugin.as_str())),
            }
        })
        .collect()
}

/// The toggletip saying the description of `option`, a plugin of the list whose id is `list`;
/// nothing if it has none.
#[component]
fn About(list: String, option: PluginOption) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let Some(text) = plugin_description(&tr, &option) else {
        return rsx! {};
    };
    // ids of plugins are namespaced by `::`
    let id = format!("{list}-about-{}", option.id.replace("::", "-"));
    rsx! {
        Toggletip { id, subject: plugin_label(&tr, &option), text }
    }
}

/// A set of plugins, each chosen or not by a checkbox, and configured once chosen.
#[component]
fn UnorderedList(
    form: Signal<PhaseForm>,
    path: Path,
    list_id: String,
    list: ListInput,
    disabled: bool,
) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    // in one order, whether chosen or not, so that a plugin stays where it is when it is clicked
    let entries: Vec<_> = list
        .set()
        .into_iter()
        .map(|entry| match entry {
            SetEntry::Chosen(item) => SetItem {
                option: item.plugin,
                text: capitalized(&plugin_label(&tr, &item.plugin)),
                state: SetState::Chosen(item.clone()),
            },
            SetEntry::Implied(option) => SetItem {
                option: *option,
                text: capitalized(&t!(
                    tr,
                    "implied-plugin",
                    plugin = plugin_label(&tr, option).as_str()
                )),
                state: SetState::Implied,
            },
            SetEntry::Offered(option) => SetItem {
                option: *option,
                text: capitalized(&plugin_label(&tr, option)),
                state: SetState::Offered,
            },
        })
        .collect();
    rsx! {
        ul { class: "plugin-set", id: "{list_id}",
            // keyed by the plugin, which a list has once, and with the same elements whether
            // chosen or not, so that its checkbox keeps the focus when it is clicked; an implied
            // one has elements of its own, since the page would not check again a checkbox just
            // unchecked whose plugin turned out to be implied, having rendered it checked before
            for entry in entries {
                li { key: "{entry.key()}",
                    {
                        let plugin = entry.option.id;
                        let chosen = match &entry.state {
                            SetState::Chosen(item) => Some(item.path.clone()),
                            _ => None,
                        };
                        let add = path.clone();
                        rsx! {
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: !matches!(entry.state, SetState::Offered),
                                    disabled: disabled || matches!(entry.state, SetState::Implied),
                                    onchange: move |_| {
                                        _ = match &chosen {
                                            Some(item) => form.write().remove(item),
                                            None => form.write().add(&add, plugin),
                                        };
                                    },
                                }
                                " {entry.text}"
                            }
                        }
                    }
                    " "
                    About { list: list_id.clone(), option: entry.option }
                    if let SetState::Chosen(item) = entry.state {
                        Fields { form, fields: item.fields, disabled }
                        Issues { id: "{id(&item.path)}-issues", issues: item.issues }
                    }
                }
            }
        }
    }
}

/// A plugin of a set, as [`UnorderedList`] shows it.
struct SetItem {
    option: PluginOption,
    state: SetState,
    /// What the set shows for it.
    text: String,
}

impl SetItem {
    /// What its element is keyed by, see [`UnorderedList`].
    fn key(&self) -> String {
        match self.state {
            SetState::Implied => format!("implied-{}", self.option.id),
            _ => self.option.id.to_owned(),
        }
    }
}

#[derive(Clone)]
enum SetState {
    Chosen(Item),
    Implied,
    Offered,
}

/// What is wrong, as a list with the id `id`, which the inputs it is about name as their
/// description; nothing if nothing is. The list takes the focus from [`focus_first_issue`].
#[component]
fn Issues(id: String, issues: Vec<Text>) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    rsx! {
        if !issues.is_empty() {
            ul { class: "issues", id: "{id}", tabindex: "-1",
                for issue in issues {
                    li { {tr.sentence(&issue)} }
                }
            }
        }
    }
}

/// What the answer of a field leads to that the operator may not want, although the phase can be
/// played with it, as a list with the id `id`; nothing if there is nothing to say.
#[component]
fn Warnings(id: String, warnings: Vec<Text>) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    rsx! {
        if !warnings.is_empty() {
            ul { class: "warnings", id: "{id}",
                for warning in warnings {
                    li { {tr.sentence(&warning)} }
                }
            }
        }
    }
}

/// Moves the focus, once the page shows what was just changed, to the first element that
/// `selectors` find, if any, e.g. to the button of the next item after one removed.
pub fn focus_later(selectors: &[&str]) {
    // ids and classes of the page, with nothing a JavaScript string would read otherwise than Rust
    let selectors = format!("{selectors:?}");
    // the second frame is after the page rendered the change, which it does in the first
    _ = document::eval(&format!(
        r#"
        requestAnimationFrame(() => requestAnimationFrame(() => {{
            for (const selector of {selectors}) {{
                const element = document.querySelector(selector);
                if (element) {{
                    element.focus();
                    break;
                }}
            }}
        }}));
        "#
    ));
}

/// Moves the focus to the first list of issues in the form whose element has the id `form`,
/// scrolled into the middle of the screen, so that the operator sees and hears what keeps the
/// phase from being played.
fn focus_first_issue(form: &str) {
    _ = document::eval(&format!(
        r##"
        const issue = document.querySelector("#{form} .issues");
        issue?.scrollIntoView({{ block: "center" }});
        issue?.focus({{ preventScroll: true }});
        "##
    ));
}

/// How the phase is played, read from the fields of its form.
#[component]
fn Summary(fields: Vec<FormField>) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    rsx! {
        dl { class: "summary",
            for field in fields {
                dt { key: "{field.path}", {capitalized(&tr.field_label(&field))} }
                dd {
                    match &field.input {
                        // each item of a list on a line of its own, with its fields below it
                        Input::List(list) if has_fields(list) => rsx! {
                            ul { class: "summary-items",
                                for item in list.items.clone() {
                                    li {
                                        span { class: "answer",
                                            {capitalized(&plugin_label(&tr, &item.plugin))}
                                        }
                                        if !item.fields.is_empty() {
                                            Summary { fields: item.fields }
                                        }
                                    }
                                }
                                for option in list.implied.clone() {
                                    li {
                                        span { class: "answer",
                                            {capitalized(&plugin_label(&tr, &option))}
                                        }
                                    }
                                }
                            }
                        },
                        input => rsx! {
                            span { class: "answer", {capitalized(&answer(&tr, input))} }
                            if let Input::Plugin(PluginInput { chosen: Some(chosen), .. }) = input
                                && !chosen.fields.is_empty()
                            {
                                Summary { fields: chosen.fields.clone() }
                            }
                        },
                    }
                }
            }
        }
    }
}

/// Whether any plugin of `list` has fields.
fn has_fields(list: &ListInput) -> bool {
    list.items.iter().any(|item| !item.fields.is_empty())
}

/// `input`'s answer as the summary reads it, without the fields of the plugins in it.
pub fn answer(tr: &Translator, input: &Input) -> String {
    match input {
        Input::Bool(Some(true)) => tr.get("yes"),
        Input::Bool(Some(false)) => tr.get("no"),
        Input::Integer {
            value: Some(value), ..
        } => value.to_string(),
        Input::Decimal {
            value: Some(value), ..
        } => tr.number(*value),
        Input::Optional {
            answered: true,
            inner,
        } => answer(tr, inner),
        Input::Plugin(PluginInput {
            chosen: Some(chosen),
            ..
        }) => plugin_label(tr, &chosen.plugin),
        Input::List(list) => {
            let labels: Vec<_> = list
                .items
                .iter()
                .map(|item| plugin_label(tr, &item.plugin))
                .chain(list.implied.iter().map(|option| plugin_label(tr, option)))
                .collect();
            if labels.is_empty() {
                tr.get("no-plugin")
            } else {
                labels.join(", ")
            }
        }
        _ => tr.get("no-plugin"),
    }
}

fn plugin_label(tr: &Translator, option: &PluginOption) -> String {
    tr.plugin(option.id, PluginText::Label, option.label)
}

fn plugin_description(tr: &Translator, option: &PluginOption) -> Option<String> {
    option.description.map(|description| {
        tr.plugin(
            description.plugin,
            PluginText::Description,
            description.english,
        )
    })
}

/// The id of the element of the field at `path`, e.g. `field-metrics-1-points_for_win`.
fn id(path: &Path) -> String {
    let path = path
        .to_string()
        .replace(['.', '[', ']'], "-")
        .replace("--", "-");
    format!("field-{}", path.trim_end_matches('-'))
}
