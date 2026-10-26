//! The phase: the form configuring it, generated from the [`FormView`] of a [`PhaseForm`], a
//! summary of how it is played, and the dialog changing it once added.

use dioxus::prelude::*;
use tournament_app::{
    form::{
        FormField, FormView, Input, Item, ListInput, Path, PhaseForm, PluginInput, PluginOption,
    },
    i18n::{Message as _, PluginText, Text, Translator},
};

use crate::{
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
        Err(error) => return rsx! { p { class: "error", {tr.text(&error)} } },
    };
    rsx! {
        form {
            onsubmit: move |event| {
                event.prevent_default();
                _ = ctx.act(|session| session.add_phase(form.read().draft()));
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
                    class: "dialog-form",
                    onsubmit: move |event| {
                        event.prevent_default();
                        match ctx.try_act(|session| session.edit_phase(form.read().draft())) {
                            Ok(_) => editing.set(None),
                            Err(refused) => error.set(Some(refused)),
                        }
                    },
                    Form { form, disabled: false }
                    p { class: "error", role: "alert",
                        if let Some(error) = error() {
                            {tr.text(&error)}
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
    let described_by = format!("{id}-description {id}-issues");
    let issues = rsx! {
        Issues { id: "{id}-issues", issues: field.issues.clone() }
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
                    min: range.min().to_string(),
                    max: range.max().to_string(),
                    step: range.multiple_of().to_string(),
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
            let shown = entered.or_else(|| value.map(|value| value.to_string()));
            let step = range
                .multiple_of()
                .map_or_else(|| "any".to_owned(), |step| step.to_string());
            rsx! {
                NumberField {
                    form, path, id, label, disabled, step,
                    value: shown,
                    min: range.min().map(|min| min.to_string()).unwrap_or_default(),
                    max: range.max().map(|max| max.to_string()).unwrap_or_default(),
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
/// what is said about it, `about`, next to its label.
#[component]
fn NumberField(
    form: Signal<PhaseForm>,
    path: Path,
    id: String,
    label: String,
    value: Option<String>,
    min: String,
    max: String,
    step: String,
    hint: String,
    disabled: bool,
    about: Element,
    children: Element,
) -> Element {
    rsx! {
        div { class: "field",
            label { r#for: "{id}", "{label}" }
            {about}
            " "
            input {
                r#type: "number",
                id: "{id}",
                inputmode: if step == "any" || step.contains('.') { "decimal" } else { "numeric" },
                value: value.unwrap_or_default(),
                min: (!min.is_empty()).then_some(min),
                max: (!max.is_empty()).then_some(max),
                step,
                disabled,
                aria_describedby: "{id}-hint {id}-description {id}-issues",
                onchange: move |event| _ = form.write().enter(&path, &event.value()),
            }
            " "
            span { class: "hint", id: "{id}-hint", "({hint})" }
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
    rsx! {
        div { class: "field",
            span { class: "label", id: "{id}-label", "{label}" }
            {about}
            " "
            Select {
                id: "{id}",
                labelled_by: "{id}-label",
                value: chosen.or_else(|| plugin.optional.then(String::new)),
                placeholder: tr.get("choose-plugin"),
                disabled,
                // the option of no plugin has no id
                on_change: move |chosen: String| {
                    let chosen = Some(chosen.as_str()).filter(|chosen| !chosen.is_empty());
                    _ = form.write().choose(&choose, chosen);
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
            if let Some(chosen) = &plugin.chosen {
                " "
                About { list: id.clone(), option: chosen.plugin }
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
    let label = plugin_label(&tr, &option);
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
    rsx! {
        ol { class: "plugin-list", id: "{id}",
            // keyed by the plugin, so that one moved keeps its element, and the focus
            for (index, item) in list.items.into_iter().enumerate() {
                OrderedItem {
                    key: "{item.plugin.id}",
                    list: id.clone(),
                    form,
                    item,
                    first: index == 0,
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
                    on_change: move |chosen: String| _ = form.write().add(&path, &chosen),
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

/// A plugin of a list whose order matters, with the buttons moving and removing it, and its
/// fields.
#[component]
fn OrderedItem(
    form: Signal<PhaseForm>,
    list: String,
    item: Item,
    first: bool,
    last: bool,
    disabled: bool,
) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    let name = plugin_label(&tr, &item.plugin);
    let (up, down, remove) = (item.path.clone(), item.path.clone(), item.path.clone());
    rsx! {
        li {
            span { class: "name", "{name}" }
            " "
            About { list, option: item.plugin }
            button {
                class: "quiet small",
                r#type: "button",
                aria_label: t!(tr, "move-up", plugin = name.as_str()),
                disabled: disabled || first,
                onclick: move |_| _ = form.write().move_up(&up),
                "↑"
            }
            button {
                class: "quiet small",
                r#type: "button",
                aria_label: t!(tr, "move-down", plugin = name.as_str()),
                disabled: disabled || last,
                onclick: move |_| _ = form.write().move_down(&down),
                "↓"
            }
            button {
                class: "quiet small",
                r#type: "button",
                aria_label: t!(tr, "remove-plugin", plugin = name.as_str()),
                disabled,
                onclick: move |_| _ = form.write().remove(&remove),
                {tr.get("remove")}
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
                text: t!(tr, "implied-plugin", plugin = plugin.as_str()),
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
    let implied = implied(&tr, &list.implied);
    rsx! {
        ul { class: "plugin-set", id: "{list_id}",
            // keyed by the plugin, which a list has once, rather than by its position: the item
            // after one removed moves to its position, and would take over the element whose
            // checkbox was just unchecked, which the page does not check again since it rendered
            // it checked before as well
            for item in list.items {
                li { key: "{item.plugin.id}",
                    {
                        let remove = item.path.clone();
                        rsx! {
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: true,
                                    disabled,
                                    onchange: move |_| _ = form.write().remove(&remove),
                                }
                                " {plugin_label(&tr, &item.plugin)}"
                            }
                        }
                    }
                    " "
                    About { list: list_id.clone(), option: item.plugin }
                    Fields { form, fields: item.fields, disabled }
                    Issues { id: "{id(&item.path)}-issues", issues: item.issues }
                }
            }
            for implied in implied {
                li { key: "{implied.option.id}",
                    label {
                        input { r#type: "checkbox", checked: true, disabled: true }
                        " {implied.text}"
                    }
                    " "
                    About { list: list_id.clone(), option: implied.option }
                }
            }
            for option in list.options {
                li { key: "{option.id}",
                    {
                        let add = path.clone();
                        rsx! {
                            label {
                                input {
                                    r#type: "checkbox",
                                    checked: false,
                                    disabled,
                                    onchange: move |_| _ = form.write().add(&add, option.id),
                                }
                                " {plugin_label(&tr, &option)}"
                            }
                        }
                    }
                    " "
                    About { list: list_id.clone(), option }
                }
            }
        }
    }
}

/// What is wrong, as a list with the id `id`, which the inputs it is about name as their
/// description; nothing if nothing is.
#[component]
fn Issues(id: String, issues: Vec<Text>) -> Element {
    let ctx = use_context::<Ctx>();
    let tr = ctx.translator.read();
    rsx! {
        if !issues.is_empty() {
            ul { class: "issues", id: "{id}",
                for issue in issues {
                    li { {tr.text(&issue)} }
                }
            }
        }
    }
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
                                        span { class: "answer", {plugin_label(&tr, &item.plugin)} }
                                        if !item.fields.is_empty() {
                                            Summary { fields: item.fields }
                                        }
                                    }
                                }
                                for option in list.implied.clone() {
                                    li {
                                        span { class: "answer", {plugin_label(&tr, &option)} }
                                    }
                                }
                            }
                        },
                        input => rsx! {
                            span { class: "answer", {answer(&tr, input)} }
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
fn answer(tr: &Translator, input: &Input) -> String {
    match input {
        Input::Bool(Some(true)) => tr.get("yes"),
        Input::Bool(Some(false)) => tr.get("no"),
        Input::Integer {
            value: Some(value), ..
        } => value.to_string(),
        Input::Decimal {
            value: Some(value), ..
        } => value.to_string(),
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

/// `label` with its first letter in upper case, as a label on the screen starts.
fn capitalized(label: &str) -> String {
    let mut chars = label.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
