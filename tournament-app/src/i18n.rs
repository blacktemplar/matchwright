//! The operator's language: what the app and a GUI on it say, translated with
//! [Fluent](https://projectfluent.org).
//!
//! A [`Translator`] holds the messages of one [`Language`]: those of this crate, which are what its
//! errors say (each is a [`Message`]), and those of a GUI, which it hands over as [`Resources`].
//! A message missing in the language is taken from English.
//!
//! Plugins are translated by keys derived from their ids, see [`PluginText`], and are shown with
//! the English of their code where no translation has them, so that a plugin needs no change to be
//! shown, and to be translated. The fields of a phase itself are translated in the same way, see
//! [`Translator::field_label`], and so are the stages a format names, see [`Translator::stage`],
//! and what a calculator's column shows, see [`Translator::cell`]. Numbers are written as the
//! language writes them, in messages too.

use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
    num::NonZeroU32,
};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use fluent_syntax::{ast, parser};
use tournament_engine::plugins::{Phrase, metrics_calculator::Cell};

use crate::{form::FormField, view::StageName};

/// A language the app speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Language {
    #[default]
    English,
    German,
}

impl Language {
    /// Every language, in the order a list to choose one from shows them.
    pub const ALL: [Self; 2] = [Self::English, Self::German];

    /// Its language tag (BCP 47), e.g. for the `lang` attribute of HTML.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::German => "de",
        }
    }

    /// Its name in itself, as a list to choose a language from shows it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::German => "Deutsch",
        }
    }

    /// The language the language tag `tag` names, whatever its region or script, e.g. German for
    /// `"de-AT"`; `None` if the app does not speak it.
    pub fn from_tag(tag: &str) -> Option<Self> {
        let language = tag.split(['-', '_']).next()?;
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.tag().eq_ignore_ascii_case(language))
    }

    /// The first language of `preferred`, language tags as a browser lists them (most preferred
    /// first), that the app speaks, or else English.
    pub fn negotiate<'a>(preferred: impl IntoIterator<Item = &'a str>) -> Self {
        preferred
            .into_iter()
            .find_map(Self::from_tag)
            .unwrap_or_default()
    }

    /// The messages of this crate in this language: its own, and those of the engine's plugins.
    fn resources(self) -> [&'static str; 2] {
        match self {
            Self::English => [
                include_str!("../locales/en/app.ftl"),
                include_str!("../locales/en/plugins.ftl"),
            ],
            Self::German => [
                include_str!("../locales/de/app.ftl"),
                include_str!("../locales/de/plugins.ftl"),
            ],
        }
    }
}

/// The messages of a consumer in each language, in Fluent's syntax, e.g. `include_str!` of a file
/// per language.
pub type Resources = fn(Language) -> &'static str;

/// Something to be said in the operator's language: the key of a Fluent message, and the values of
/// its variables, and possibly a detail, which is not translated.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    key: &'static str,
    args: Vec<(&'static str, Arg)>,
    detail: Option<String>,
}

impl Text {
    /// The message `key`.
    pub const fn new(key: &'static str) -> Self {
        Self {
            key,
            args: Vec::new(),
            detail: None,
        }
    }

    /// This text with the variable `name` set to `value`.
    #[must_use]
    pub fn with(mut self, name: &'static str, value: impl Into<Arg>) -> Self {
        self.args.push((name, value.into()));
        self
    }

    /// This text with `detail`, which says more for whoever looks into it, e.g. where a file
    /// stops being readable, as a library says it, in English.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The key of its message.
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// Its detail, see [`Self::with_detail`].
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

/// The value of a variable of a [`Text`].
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    String(String),
    Number(f64),
    /// The label of the plugin `id` in the language, else `english`, see [`Translator::plugin`].
    Plugin {
        id: &'static str,
        english: &'static str,
    },
    /// The labels of plugins, each as [`Self::Plugin`] by id and English label, joined by commas.
    Plugins(Vec<(&'static str, &'static str)>),
}

impl From<String> for Arg {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for Arg {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<f64> for Arg {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<u32> for Arg {
    fn from(value: u32) -> Self {
        Self::Number(value.into())
    }
}

impl From<NonZeroU32> for Arg {
    fn from(value: NonZeroU32) -> Self {
        value.get().into()
    }
}

impl From<usize> for Arg {
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts of what is on a screen are far below 2^52"
    )]
    fn from(value: usize) -> Self {
        Self::Number(value as f64)
    }
}

/// Something the operator is told, e.g. why an action was refused.
pub trait Message {
    /// What it says.
    fn text(&self) -> Text;
}

/// What is translated of a plugin, in a language that has it. Each is an attribute of the Fluent
/// message `plugin-<id>`, with each `::` of the id replaced by `-`:
///
/// ```text
/// plugin-tournament_engine-points =
///     .label = Punkte
///     .description = für jeden Sieg, jedes Unentschieden und jedes Freilos
///     .column = Punkte
///     .field-points_for_bye = Punkte für ein Freilos
///     .field-points_for_bye-description = eine Runde ohne Spiel, …
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginText<'a> {
    /// Its label, `.label`.
    Label,
    /// Its description, `.description`.
    Description,
    /// The header of the column of a calculator's metric, `.column`.
    Column,
    /// The label of its field of this name, `.field-<name>`.
    Field(&'a str),
    /// The description of its field of this name, `.field-<name>-description`.
    FieldDescription(&'a str),
    /// The name of a format's stage of this key, `.stage-<key>`, with the stage's numbers as its
    /// variables, see [`Translator::stage`] and
    /// [`Stage::key`](tournament_engine::plugins::phase_format::Stage::key).
    Stage(&'a str),
    /// The phrase of this key in a cell of a calculator's column, `.cell-<key>`, with the phrase's
    /// numbers as its variables, see [`Translator::cell`].
    Cell(&'a str),
}

impl PluginText<'_> {
    /// The key of the message of the plugin `id`.
    fn key(id: &str) -> String {
        format!("plugin-{}", id.replace("::", "-"))
    }

    /// The name of its attribute.
    fn attribute(self) -> String {
        match self {
            Self::Label => "label".to_owned(),
            Self::Description => "description".to_owned(),
            Self::Column => "column".to_owned(),
            Self::Field(name) => format!("field-{name}"),
            Self::FieldDescription(name) => format!("field-{name}-description"),
            Self::Stage(key) => format!("stage-{key}"),
            Self::Cell(key) => format!("cell-{key}"),
        }
    }
}

/// The message whose attributes translate the fields of a phase itself, as those of a plugin's
/// message translate its fields, see [`Translator::field_label`].
const PHASE: &str = "phase";

/// The messages of one language.
pub struct Translator {
    language: Language,
    bundle: FluentBundle<FluentResource>,
    /// Where a message missing in the language comes from; `None` for English itself.
    english: Option<FluentBundle<FluentResource>>,
}

impl Translator {
    /// The messages of this crate and of `resources` in `language`.
    pub fn new(language: Language, resources: &[Resources]) -> Self {
        Self {
            language,
            bundle: bundle(language, resources),
            english: (language != Language::English).then(|| bundle(Language::English, resources)),
        }
    }

    /// The language it speaks.
    pub fn language(&self) -> Language {
        self.language
    }

    /// `text` in the language, or else in English, or else its key.
    ///
    /// The value of each variable is enclosed in the invisible marks U+2068 and U+2069, which
    /// isolate its writing direction from the text around it. A variable `text` does not set shows
    /// as `{$name}`.
    pub fn text(&self, text: &Text) -> String {
        let mut args = FluentArgs::new();
        for (name, value) in &text.args {
            args.set(
                *name,
                match value {
                    Arg::String(string) => FluentValue::from(string.as_str()),
                    Arg::Number(number) => FluentValue::from(*number),
                    Arg::Plugin { id, english } => {
                        FluentValue::from(self.plugin(id, PluginText::Label, english))
                    }
                    Arg::Plugins(plugins) => FluentValue::from(
                        plugins
                            .iter()
                            .map(|(id, english)| self.plugin(id, PluginText::Label, english))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                },
            );
        }
        self.format(text.key, None, Some(&args))
            .unwrap_or_else(|| text.key.to_owned())
    }

    /// `text` as a sentence of its own, as [`Self::text`] says it beginning with a capital letter
    /// and ending with a full stop, e.g. an error, which says itself as part of a sentence.
    pub fn sentence(&self, text: &Text) -> String {
        let said = self.text(text);
        let isolating = |char: char| matches!(char, '\u{2068}' | '\u{2069}');
        // a text beginning with a variable begins with the mark isolating it
        let (marks, rest) = said.split_at(said.find(|char| !isolating(char)).unwrap_or(said.len()));
        let mut rest = rest.chars();
        let mut sentence = marks.to_owned();
        sentence.extend(rest.next().into_iter().flat_map(char::to_uppercase));
        sentence.push_str(rest.as_str());
        if !sentence
            .trim_end_matches(isolating)
            .ends_with(['.', '!', '?', '…'])
        {
            sentence.push('.');
        }
        sentence
    }

    /// The message `key`, which has no variables, see [`Self::text`].
    pub fn get(&self, key: &'static str) -> String {
        self.text(&Text::new(key))
    }

    /// What `message` says, see [`Self::text`].
    pub fn message(&self, message: &(impl Message + ?Sized)) -> String {
        self.text(&message.text())
    }

    /// `text` of the plugin `id` in the language, or else `english`, what the plugin's code says.
    pub fn plugin(&self, id: &str, text: PluginText<'_>, english: &str) -> String {
        self.format(&PluginText::key(id), Some(&text.attribute()), None)
            .unwrap_or_else(|| english.to_owned())
    }

    /// The name of `stage` in the language, as the [`PluginText::Stage`] of its format with its
    /// numbers set, or else in English, as the format's code names it.
    pub fn stage(&self, stage: &StageName) -> String {
        self.phrase(
            stage.format,
            PluginText::Stage(stage.stage.key()),
            &stage.stage,
        )
    }

    /// What `cell` of the column of the calculator `calculator` shows in the language: a number as
    /// the language writes it, see [`Self::number`], and a phrase as the [`PluginText::Cell`] of
    /// the calculator with its numbers set, or else in English, as the calculator's code says it.
    pub fn cell(&self, calculator: &str, cell: &Cell) -> String {
        match cell {
            Cell::Number(number) => self.number(number.get()),
            Cell::Text(phrase) => self.phrase(calculator, PluginText::Cell(phrase.key()), phrase),
        }
    }

    /// `number` as the language writes it, e.g. `0,5` in German.
    pub fn number(&self, number: f64) -> String {
        written(self.language, &number.to_string())
    }

    /// `phrase` of the plugin `id` as its `text` with the phrase's numbers set, or else in English.
    fn phrase(&self, id: &str, text: PluginText<'_>, phrase: &Phrase) -> String {
        let mut args = FluentArgs::new();
        for (name, value) in phrase.numbers() {
            args.set(*name, *value);
        }
        self.format(&PluginText::key(id), Some(&text.attribute()), Some(&args))
            .unwrap_or_else(|| phrase.to_string())
    }

    /// The label of `field` in the language, or else its English one. A field of a plugin is
    /// translated as the plugin's [`PluginText::Field`]; a field of the phase itself, which is no
    /// plugin's, as the attribute `.field-<name>` of the message `phase`.
    pub fn field_label(&self, field: &FormField) -> String {
        self.field_text(field, PluginText::Field(field.name), field.label)
    }

    /// The description of `field`, if it has one, translated as [`Self::field_label`] is with
    /// [`PluginText::FieldDescription`].
    pub fn field_description(&self, field: &FormField) -> Option<String> {
        let english = field.description?;
        Some(self.field_text(field, PluginText::FieldDescription(field.name), english))
    }

    fn field_text(&self, field: &FormField, text: PluginText<'_>, english: &str) -> String {
        match field.plugin {
            Some(plugin) => self.plugin(plugin, text, english),
            None => self
                .format(PHASE, Some(&text.attribute()), None)
                .unwrap_or_else(|| english.to_owned()),
        }
    }

    /// The message `key`, or its attribute `attribute`, in the language or else in English.
    fn format(
        &self,
        key: &str,
        attribute: Option<&str>,
        args: Option<&FluentArgs>,
    ) -> Option<String> {
        iter::once(&self.bundle)
            .chain(&self.english)
            .find_map(|bundle| format(bundle, key, attribute, args))
    }
}

/// Checks the messages of a consumer for a test: each language's have to parse, give no key twice,
/// none of the app's own keys, and have the same messages with the same attributes as English.
///
/// # Errors
/// Returns what is wrong, one line each.
pub fn check(resources: Resources) -> Result<(), Vec<String>> {
    let mut problems = check_languages(resources).err().unwrap_or_default();
    // the bundle keeps the first message of a key, so the app's would hide the consumer's
    for language in Language::ALL {
        let tag = language.tag();
        let own = messages(resources(language), tag, &mut Vec::new());
        for source in language.resources() {
            let apps = messages(source, tag, &mut Vec::new());
            for key in own.keys().filter(|key| apps.contains_key(*key)) {
                problems.push(format!("{tag}: {key} is a message of the app"));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Checks `resources` as [`check`] does, but for whether they give keys of the app's, e.g. the
/// app's own.
fn check_languages(resources: Resources) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let english = messages(
        resources(Language::English),
        Language::English.tag(),
        &mut problems,
    );
    for language in Language::ALL {
        if language == Language::English {
            continue;
        }
        let tag = language.tag();
        let translated = messages(resources(language), tag, &mut problems);
        for (key, attributes) in &english {
            match translated.get(key) {
                None => problems.push(format!("{tag}: {key} is missing")),
                Some(translated) if translated != attributes => problems.push(format!(
                    "{tag}: {key} has the attributes {translated:?} rather than {attributes:?}"
                )),
                Some(_) => {}
            }
        }
        for key in translated.keys().filter(|key| !english.contains_key(*key)) {
            problems.push(format!("{tag}: {key} is not in English"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The keys of the messages of `source`, in the language `tag`, each with its attributes.
fn messages(
    source: &'static str,
    tag: &str,
    problems: &mut Vec<String>,
) -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    let resource = parser::parse(source).unwrap_or_else(|(resource, errors)| {
        for error in errors {
            problems.push(format!("{tag}: does not parse: {error:?}"));
        }
        resource
    });
    let mut messages = BTreeMap::new();
    for entry in resource.body {
        let (key, attributes) = match entry {
            ast::Entry::Message(message) => (message.id.name, message.attributes),
            ast::Entry::Term(term) => (term.id.name, term.attributes),
            _ => continue,
        };
        let attributes = attributes
            .iter()
            .map(|attribute| attribute.id.name)
            .collect();
        if messages.insert(key, attributes).is_some() {
            problems.push(format!("{tag}: {key} is given twice"));
        }
    }
    messages
}

/// The messages of this crate and of `resources` in `language`.
fn bundle(language: Language, resources: &[Resources]) -> FluentBundle<FluentResource> {
    let tag = language
        .tag()
        .parse()
        .expect("the tag of a language is a language identifier");
    // Fluent wraps each value put into a message in the invisible marks U+2068 and U+2069, which
    // isolate its writing direction, so that a name written right to left keeps its place in the
    // sentence; they are kept, and tests comparing texts ignore them
    let mut bundle = FluentBundle::new(vec![tag]);
    // Fluent writes every number as English does; a formatter is a function, which cannot capture
    // the language
    if language == Language::German {
        bundle.set_formatter(Some(|value, _| match value {
            FluentValue::Number(number) => Some(written(Language::German, &number.as_string())),
            _ => None,
        }));
    }
    let sources = language
        .resources()
        .into_iter()
        .chain(resources.iter().map(|resources| resources(language)));
    for source in sources {
        // a resource that does not parse still has the messages that do, and a key given twice
        // keeps its first message; the tests of the resources check that neither happens
        let resource =
            FluentResource::try_new(source.to_owned()).unwrap_or_else(|(resource, _)| resource);
        _ = bundle.add_resource(resource);
    }
    bundle
}

/// The number `english`, as English writes it, as `language` writes it.
fn written(language: Language, english: &str) -> String {
    match language {
        Language::English => english.to_owned(),
        Language::German => english.replace('.', ","),
    }
}

/// The message `key` of `bundle`, or its attribute `attribute`, if it has it.
fn format(
    bundle: &FluentBundle<FluentResource>,
    key: &str,
    attribute: Option<&str>,
    args: Option<&FluentArgs>,
) -> Option<String> {
    let message = bundle.get_message(key)?;
    let pattern = match attribute {
        None => message.value()?,
        Some(attribute) => message.get_attribute(attribute)?.value(),
    };
    // what goes wrong, a variable not set, is written into the text
    let mut errors = Vec::new();
    Some(
        bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned(),
    )
}

#[cfg(test)]
mod tests;
