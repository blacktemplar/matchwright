//! The operator's language: what the app and a GUI on it say, translated with
//! [Fluent](https://projectfluent.org).
//!
//! A [`Translator`] holds the messages of one [`Language`]: those of this crate, which are what its
//! errors say (each is a [`Message`]), and those of a GUI, which it hands over as [`Resources`].
//! A message missing in the language is taken from English.
//!
//! Plugins are translated by keys derived from their ids, see [`PluginText`], and are shown with
//! the English of their code where no translation has them, so that a plugin needs no change to be
//! shown, and to be translated.

use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
    num::NonZeroU32,
};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use fluent_syntax::{ast, parser};

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
/// its variables.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    key: &'static str,
    args: Vec<(&'static str, Arg)>,
}

impl Text {
    /// The message `key`.
    pub const fn new(key: &'static str) -> Self {
        Self {
            key,
            args: Vec::new(),
        }
    }

    /// This text with the variable `name` set to `value`.
    #[must_use]
    pub fn with(mut self, name: &'static str, value: impl Into<Arg>) -> Self {
        self.args.push((name, value.into()));
        self
    }

    /// The key of its message.
    pub fn key(&self) -> &'static str {
        self.key
    }
}

/// The value of a variable of a [`Text`].
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    String(String),
    Number(f64),
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
        }
    }
}

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
                },
            );
        }
        self.format(text.key, None, Some(&args))
            .unwrap_or_else(|| text.key.to_owned())
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
/// and have the same messages with the same attributes as English.
///
/// # Errors
/// Returns what is wrong, one line each.
pub fn check(resources: Resources) -> Result<(), Vec<String>> {
    /// The keys of the messages of `resources` in `language`, each with its attributes.
    fn messages(
        resources: Resources,
        language: Language,
        problems: &mut Vec<String>,
    ) -> BTreeMap<&'static str, BTreeSet<&'static str>> {
        let tag = language.tag();
        let resource = parser::parse(resources(language)).unwrap_or_else(|(resource, errors)| {
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

    let mut problems = Vec::new();
    let english = messages(resources, Language::English, &mut problems);
    for language in Language::ALL {
        if language == Language::English {
            continue;
        }
        let tag = language.tag();
        let translated = messages(resources, language, &mut problems);
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
