//! What a plugin has to be told in order to be built, so that a program can ask for it without
//! knowing the plugin.
//!
//! A [`Configurable`] states its questions as a [`Schema`] — a list of [`Field`]s, each taking an
//! answer of one [`FieldKind`] and offering a default — and is deserialized from the answers, see
//! [`Config`]. A plugin registers in the [catalog](crate::catalog) with its schema, so that a
//! program reads the questions of a plugin before it is chosen and never names one.

use std::collections::BTreeSet;

pub use plugin_factory_derive::Configurable;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    Plugin,
    catalog::{AnyAxis, Axis},
    config::{self, Config, ConfigError, Path, PluginConfig, Value},
};

/// Something configured by answering the fields of its [`Schema`]: a plugin, or the composite a
/// program configures plugins into.
///
/// Only derived: for a plugin by [`derive@Plugin`](crate::Plugin), for a composite by
/// [`derive@Configurable`]. Implementing it by hand is not supported.
///
/// It is built by deserializing its answers, which have to answer every field; a field's
/// [default](Field::default_value) is only offered, never filled in. A struct without fields may
/// be a unit struct, which is deserialized from `null` rather than from an empty config. An
/// answer it rejects, e.g. through `#[serde(deserialize_with = ..)]` or a field type's
/// `#[serde(try_from = ..)]`, is reported as [`ConfigError::Rejected`] with the path of the
/// answer.
///
/// A field written as an `Option` is told apart from one answered with nothing even where it is
/// deserialized without its schema, e.g. when loaded: it needs a function deserializing it, so
/// that `serde` rejects it missing, or a `#[serde(default)]` it is read missing as.
///
/// ```compile_fail
/// use plugin_factory::schema::Configurable;
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Configurable)]
/// struct Limits {
///     #[plugin(label = "Cap")]
///     cap: Option<u32>,
/// }
/// ```
///
/// # Examples
/// ```
/// use plugin_factory::schema::Configurable;
/// use serde::Deserialize;
/// use serde_json::json;
///
/// #[derive(Deserialize, Configurable)]
/// struct Limits {
///     #[serde(deserialize_with = "Option::deserialize")]
///     #[plugin(label = "Cap")]
///     cap: Option<u32>,
/// }
///
/// assert!(serde_json::from_value::<Limits>(json!({"cap": null})).is_ok());
/// assert!(serde_json::from_value::<Limits>(json!({})).is_err());
/// ```
pub trait Configurable: Sized + DeserializeOwned + 'static {
    /// Whether it is a unit struct, deserialized from `null`. Set by the derive; not part of the
    /// API.
    #[doc(hidden)]
    const IS_UNIT: bool = false;

    /// What has to be answered in order to build it.
    fn schema() -> Schema;

    /// The plugins held by its fields, in schema order; none by default.
    ///
    /// For a plugin, what they declare is part of what it declares.
    fn plugins(&self) -> Vec<&dyn Plugin> {
        Vec::new()
    }
}

/// A type a field of a derived plugin can have, see [`derive@Plugin`]: what kind of answer it
/// takes.
///
/// Implemented for `bool`, the integer and floating point types, `Box<dyn Trait>` for the trait of
/// a plugin axis, and `Option`, `Vec`, [`Set`] and `BTreeSet` of any of them. A `Vec` takes an
/// ordered list, a `Set` or `BTreeSet` an unordered one. A type that deserializes from one of
/// these answers, such as a newtype checking its value, implements it to be a field.
pub trait FieldType {
    /// The kind of answer a field of this type takes.
    fn kind() -> FieldKind;

    /// Adds the plugins `self` holds to `plugins`; none by default. Called by the derive; not
    /// part of the API.
    #[doc(hidden)]
    fn plugins<'a>(&'a self, _plugins: &mut Vec<&'a dyn Plugin>) {}
}

impl FieldType for bool {
    fn kind() -> FieldKind {
        FieldKind::Bool
    }
}

/// Implements [`FieldType`] for each of the types, as answered by `$kind`.
macro_rules! field_types {
    ($kind:ident: $($type:ty),*) => {
        $(
            impl FieldType for $type {
                fn kind() -> FieldKind {
                    FieldKind::$kind
                }
            }
        )*
    };
}

field_types!(Integer: i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
field_types!(Decimal: f32, f64);

impl<A: Axis + ?Sized> FieldType for Box<A> {
    fn kind() -> FieldKind {
        FieldKind::plugin::<A>()
    }

    fn plugins<'a>(&'a self, plugins: &mut Vec<&'a dyn Plugin>) {
        plugins.push(self.as_ref().as_plugin());
    }
}

impl<T: FieldType> FieldType for Option<T> {
    fn kind() -> FieldKind {
        FieldKind::optional(T::kind())
    }

    fn plugins<'a>(&'a self, plugins: &mut Vec<&'a dyn Plugin>) {
        if let Some(answer) = self {
            answer.plugins(plugins);
        }
    }
}

impl<T: FieldType> FieldType for Vec<T> {
    fn kind() -> FieldKind {
        FieldKind::list(T::kind())
    }

    fn plugins<'a>(&'a self, plugins: &mut Vec<&'a dyn Plugin>) {
        for answer in self {
            answer.plugins(plugins);
        }
    }
}

impl<T: FieldType> FieldType for Set<T> {
    fn kind() -> FieldKind {
        FieldKind::set(T::kind())
    }

    fn plugins<'a>(&'a self, plugins: &mut Vec<&'a dyn Plugin>) {
        for answer in self {
            answer.plugins(plugins);
        }
    }
}

// `HashSet` is left out on purpose: it serializes in an order that differs from run to run.
impl<T: FieldType + Ord> FieldType for BTreeSet<T> {
    fn kind() -> FieldKind {
        FieldKind::set(T::kind())
    }

    fn plugins<'a>(&'a self, plugins: &mut Vec<&'a dyn Plugin>) {
        for answer in self {
            answer.plugins(plugins);
        }
    }
}

/// The answers to a field taking an unordered list, for elements that cannot be put in a
/// `BTreeSet`, such as plugins or numbers with a fractional part.
///
/// It holds the answers in the order they are given in and is (de)serialized as that list. A
/// config giving an answer twice is rejected with [`ConfigError::Duplicate`] when it is checked
/// against its schema, as it is when a plugin is built from it; deserializing it on its own,
/// e.g. when it is loaded, does not check that.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Set<T>(Vec<T>);

impl<T> Set<T> {
    /// The answers, in the order they are given in.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    /// How many answers it holds.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it holds no answer.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T> Default for Set<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> IntoIterator for Set<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Set<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Everything a [`Configurable`] has to be told, in the order it is asked in.
///
/// A field the derive leaves out with `#[plugin(skip)]` is not part of it: a config answering
/// such a field is not rejected, but the answer is dropped.
#[derive(Debug, Clone)]
pub struct Schema {
    fields: Vec<Field>,
    skipped: Vec<&'static str>,
}

impl Schema {
    /// A schema asking for nothing. Called by the derive; not part of the API.
    #[doc(hidden)]
    pub const fn new() -> Self {
        Self {
            fields: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// This schema with `field` asked after the fields it holds already. Called by the derive;
    /// not part of the API.
    #[doc(hidden)]
    #[must_use]
    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    /// This schema with the field `name` left out, see [`Schema`]. Called by the derive; not part
    /// of the API.
    #[doc(hidden)]
    #[must_use]
    pub fn skip(mut self, name: &'static str) -> Self {
        self.skipped.push(name);
        self
    }

    /// The fields of this schema, in the order they are asked in.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The field named `name`, or [`None`] if the schema has none.
    pub fn field_named(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name() == name)
    }

    /// `config`, the config at `at`, checked against this schema, and so the config of every
    /// plugin answering one of its fields against the schema of that plugin, down the whole tree.
    ///
    /// If `complete`, every field has to be answered, as for building a [`Configurable`];
    /// otherwise a field may be left unanswered. A plugin answer is written as
    /// [`AnyAxis::walk`] writes it. An answer to a field left out of the schema is dropped.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if a config answers a field its schema neither has
    /// nor leaves out, [`ConfigError::WrongKind`] if an answer is not one its field takes,
    /// [`ConfigError::UnknownPlugin`] if a plugin answering a field is not registered for its
    /// axis, [`ConfigError::Duplicate`] if a list gives an answer twice, as
    /// [`FieldKind::List`] counts answers as the same, and, if `complete`,
    /// [`ConfigError::Unanswered`] if a field is unanswered, each with its path from the root.
    pub(crate) fn walk(
        &self,
        config: &Config,
        at: &Path,
        complete: bool,
    ) -> Result<Config, ConfigError> {
        if let Some((answered, _)) = config.answers().find(|(answered, _)| {
            self.field_named(answered).is_none()
                && !self.skipped.iter().any(|skipped| skipped == answered)
        }) {
            return Err(ConfigError::UnknownField {
                path: at.clone().field(answered),
            });
        }
        let mut walked = Config::new();
        for field in self.fields() {
            let path = at.clone().field(field.name());
            let answer = match config.get(field.name()) {
                Some(answer) => answer,
                None if !complete => continue,
                None => return Err(ConfigError::Unanswered { path }),
            };
            walked.set(field.name(), field.kind().walk(answer, &path, complete)?);
        }
        Ok(walked)
    }
}

/// One question a [`Configurable`] asks, i.e. what is answered in a [`Config`] under its name.
///
/// Every field has to be answered, see [`PluginConfig::build`]; its default is what a UI offers
/// as the answer.
#[derive(Debug, Clone)]
pub struct Field {
    name: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    kind: FieldKind,
    default: Option<Value>,
}

impl Field {
    /// A field answered by an answer of kind `kind`, named `name` and labelled `label`. Called by
    /// the derive; not part of the API.
    #[doc(hidden)]
    pub const fn new(name: &'static str, label: &'static str, kind: FieldKind) -> Self {
        Self {
            name,
            label,
            description: None,
            kind,
            default: None,
        }
    }

    /// This field with `description` as the help text of its question. Called by the derive; not
    /// part of the API.
    #[doc(hidden)]
    #[must_use]
    pub const fn described(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }

    /// This field with `default` as the answer it offers. Called by the derive; not part of the
    /// API.
    #[doc(hidden)]
    #[must_use]
    pub fn with_default(mut self, default: impl Into<Value>) -> Self {
        self.default = Some(default.into());
        self
    }

    /// What this field is called in a [`Config`].
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// What an operator answering this field reads.
    pub fn label(&self) -> &'static str {
        self.label
    }

    /// What an operator answering this field reads about what the answer means, if anything.
    pub fn description(&self) -> Option<&'static str> {
        self.description
    }

    /// What kind of answer this field takes.
    pub fn kind(&self) -> &FieldKind {
        &self.kind
    }

    /// The answer this field offers, or [`None`] if it offers none. Offering an answer does not
    /// make the field optional: it is still to be answered.
    pub fn default_value(&self) -> Option<&Value> {
        self.default.as_ref()
    }
}

/// What kind of answer a [`Field`] takes, i.e. which [`Value`]s answer it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// Yes or no, answered by [`Value::Bool`].
    Bool,
    /// A whole number, answered by a [`Value::Number`] that fits an `i64`.
    Integer,
    /// A number with a fractional part, answered by any [`Value::Number`].
    Decimal,
    /// One plugin of the axis, answered by a [`PluginConfig`] naming a plugin registered for it.
    Plugin(AnyAxis),
    /// Any number of distinct answers of the kind `element`, answered by a [`Value::Array`].
    ///
    /// Giving the same answer twice is rejected with [`ConfigError::Duplicate`]. Two plugins count
    /// as the same if they are the same plugin, whatever they are configured with, even in a
    /// config that is still incomplete. Other answers count as the same only in a complete
    /// config, numbers if they are equal in value, e.g. `1` and `1.0`. Unless `ordered`, the list
    /// is a set: the order of its answers means nothing.
    List {
        element: Box<FieldKind>,
        ordered: bool,
    },
    /// An answer of the kind or nothing, answered by an answer of the kind or by [`Value::Null`].
    ///
    /// Being answered with nothing is being answered. An optional answer of an optional answer is
    /// just an optional answer.
    Optional(Box<FieldKind>),
}

impl FieldKind {
    /// One plugin of the axis `A`.
    pub const fn plugin<A: Axis + ?Sized>() -> Self {
        Self::Plugin(AnyAxis::of::<A>())
    }

    /// Any number of distinct answers of the kind `element`, in an order that matters.
    pub fn list(element: Self) -> Self {
        Self::List {
            element: Box::new(element),
            ordered: true,
        }
    }

    /// Any number of distinct answers of the kind `element`, in an order that does not matter.
    pub fn set(element: Self) -> Self {
        Self::List {
            element: Box::new(element),
            ordered: false,
        }
    }

    /// An answer of the kind `answer` or nothing.
    pub fn optional(answer: Self) -> Self {
        Self::Optional(Box::new(answer))
    }

    /// The axis of the plugins this kind of answer is made of, through lists and optional
    /// answers, or [`None`] if it takes no plugin.
    pub fn axis(&self) -> Option<AnyAxis> {
        match self {
            Self::Plugin(axis) => Some(*axis),
            Self::List { element: inner, .. } | Self::Optional(inner) => inner.axis(),
            Self::Bool | Self::Integer | Self::Decimal => None,
        }
    }

    /// `answer`, the answer at `at`, checked against this kind, with the config of every plugin
    /// in it walked as [`Schema::walk`] walks it.
    ///
    /// # Errors
    /// Returns what [`Schema::walk`] returns.
    pub(crate) fn walk(
        &self,
        answer: &Value,
        at: &Path,
        complete: bool,
    ) -> Result<Value, ConfigError> {
        match (self, answer) {
            (Self::Bool, Value::Bool(_)) | (Self::Optional(_), Value::Null) => Ok(answer.clone()),
            (Self::Integer, Value::Number(number)) if number.is_i64() => Ok(answer.clone()),
            (Self::Decimal, Value::Number(_)) => Ok(answer.clone()),
            (Self::Optional(inner), _) => inner.walk(answer, at, complete),
            (Self::List { element, .. }, Value::Array(answers)) => {
                let walked = answers
                    .iter()
                    .enumerate()
                    .map(|(position, answer)| {
                        element.walk(answer, &at.clone().index(position), complete)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let duplicate = (1..walked.len()).find(|&position| {
                    walked[..position]
                        .iter()
                        .any(|earlier| same_element(element, earlier, &walked[position], complete))
                });
                if let Some(position) = duplicate {
                    return Err(ConfigError::Duplicate {
                        path: at.clone().index(position),
                    });
                }
                Ok(Value::Array(walked))
            }
            (Self::Plugin(axis), _) => match PluginConfig::from_answer(answer) {
                Some(plugin) => axis.walk(&plugin, at, complete),
                None => Err(self.wrong_kind(answer, at)),
            },
            _ => Err(self.wrong_kind(answer, at)),
        }
    }

    /// The error of `answer`, the answer at `at`, not being of this kind.
    fn wrong_kind(&self, answer: &Value, at: &Path) -> ConfigError {
        ConfigError::WrongKind {
            path: at.clone(),
            expected: self.name(),
            found: config::name_of(answer),
        }
    }

    /// What this kind of answer is called in an error message.
    fn name(&self) -> &'static str {
        match self {
            Self::Bool => "yes or no",
            Self::Integer => "a whole number",
            Self::Decimal => "a number",
            Self::Plugin(_) => "one plugin",
            Self::List { .. } => "a list",
            Self::Optional(_) => "an optional answer",
        }
    }
}

/// Whether `a` and `b`, walked answers of the kind `element` in one list, count as the same
/// answer: two plugins if they are the same plugin, whatever they are configured with, and,
/// only if `complete`, anything else as [`same_answer`] compares it.
fn same_element(element: &FieldKind, a: &Value, b: &Value, complete: bool) -> bool {
    match (plugin_id(element, a), plugin_id(element, b)) {
        (Some(a), Some(b)) => a == b,
        // an incomplete answer, e.g. a list in the list, may still become different from an
        // equal one, while a plugin stays the plugin it is
        _ => complete && same_answer(a, b),
    }
}

/// The id of the plugin `answer`, a walked answer of `kind`, is, or [`None`] if it is none.
fn plugin_id<'a>(kind: &FieldKind, answer: &'a Value) -> Option<&'a Value> {
    match kind {
        FieldKind::Plugin(_) => answer.get("type"),
        FieldKind::Optional(inner) => plugin_id(inner, answer),
        _ => None,
    }
}

/// Whether `a` and `b` are the same answer, with numbers compared by value so that `1` is the
/// same as `1.0`. The answers in a list are compared in order.
fn same_answer(a: &Value, b: &Value) -> bool {
    match (a, b) {
        // two whole numbers are compared exactly, as an `f64` cannot hold every `i64`
        (Value::Number(a), Value::Number(b)) => {
            a == b || ((a.is_f64() || b.is_f64()) && a.as_f64() == b.as_f64())
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_answer(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(name, a)| b.get(name).is_some_and(|b| same_answer(a, b)))
        }
        _ => a == b,
    }
}

/// Schemas and the configs answering them, tested on an axis of their own so that they do not
/// depend on what the plugins of a program happen to register.
#[cfg(test)]
mod tests {
    use serde::{Deserialize, Deserializer, Serialize, de::Error};
    use serde_json::json;

    use super::*;
    use crate::{Plugin, PluginType, axis, catalog::PluginEntry, register};

    /// A plugin axis whose plugins are nothing but their configuration, reported as a summary so
    /// that a built plugin can be checked without being downcast.
    #[axis]
    trait Configured: Plugin {
        fn summary(&self) -> String;
    }

    /// A plugin with a field of every scalar kind and one that may be answered with nothing, all
    /// with a default.
    #[derive(Debug, PartialEq, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "points",
        label = "points",
        description = "a plugin with a parameter of every scalar kind",
        recommends = Self::recommends
    )]
    struct Points {
        #[serde(default = "one", deserialize_with = "non_negative")]
        #[plugin(
            label = "Points for a win",
            description = "what winning a match is worth"
        )]
        for_win: f64,
        #[serde(default = "three")]
        #[plugin(label = "Rounds")]
        rounds: i64,
        #[serde(default = "yes")]
        #[plugin(label = "Count byes")]
        counts_byes: bool,
        #[serde(default = "half")]
        #[plugin(label = "Points for a bye")]
        for_bye: Option<f64>,
    }

    fn one() -> f64 {
        1.0
    }

    fn three() -> i64 {
        3
    }

    fn yes() -> bool {
        true
    }

    fn half() -> Option<f64> {
        Some(0.5)
    }

    fn non_negative<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let points = f64::deserialize(deserializer)?;
        if points < 0.0 {
            return Err(D::Error::custom("the points for a win cannot be negative"));
        }
        Ok(points)
    }

    impl Points {
        fn recommends(&self) -> Vec<PluginConfig> {
            vec![PluginConfig::of::<Matches>()]
        }
    }

    #[register]
    impl Configured for Points {
        fn summary(&self) -> String {
            let Self {
                for_win,
                rounds,
                counts_byes,
                for_bye,
            } = self;
            let for_bye = for_bye.map_or_else(|| "none".to_owned(), |for_bye| for_bye.to_string());
            format!("points({for_win}, {rounds}, {counts_byes}, {for_bye})")
        }
    }

    /// A plugin asking for nothing.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "matches",
        label = "matches",
        description = "a plugin without parameters"
    )]
    struct Matches;

    #[register]
    impl Configured for Matches {
        fn summary(&self) -> String {
            "matches".to_owned()
        }
    }

    /// A plugin with a field without a default, so that it has to be configured, and one taking a
    /// list of numbers.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "threshold",
        label = "threshold",
        description = "a plugin that has to be configured"
    )]
    struct Threshold {
        #[plugin(label = "Matches at least")]
        least: i64,
        #[serde(default)]
        #[plugin(label = "Weight per round")]
        weights: Vec<f64>,
    }

    #[register]
    impl Configured for Threshold {
        fn summary(&self) -> String {
            let Self { least, weights } = self;
            format!("threshold({least}, {weights:?})")
        }
    }

    /// A plugin made of other plugins, i.e. one with a field taking one plugin, one taking a list
    /// of them and one taking a plugin or nothing.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "ranker",
        label = "ranker",
        description = "a plugin made of other plugins"
    )]
    struct Ranker {
        #[serde(default = "matches")]
        #[plugin(label = "Rank by")]
        by: Box<dyn Configured>,
        #[serde(default)]
        #[plugin(label = "Tie breakers")]
        tie_breakers: Vec<Box<dyn Configured>>,
        #[serde(deserialize_with = "Option::deserialize")]
        #[plugin(label = "Fall back to")]
        fallback: Option<Box<dyn Configured>>,
    }

    fn matches() -> Box<dyn Configured> {
        Box::new(Matches)
    }

    /// A plugin with fields taking unordered lists: of whole numbers, of numbers and of plugins.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(id = "pool", label = "pool", description = "a plugin taking sets")]
    struct Pool {
        #[plugin(label = "Rounds")]
        rounds: BTreeSet<i64>,
        #[plugin(label = "Shares")]
        shares: Set<f64>,
        #[plugin(label = "Members")]
        members: Set<Box<dyn Configured>>,
    }

    #[register]
    impl Configured for Pool {
        fn summary(&self) -> String {
            let members: Vec<_> = self.members.iter().map(|member| member.summary()).collect();
            format!(
                "pool({:?}, {:?}, [{}])",
                self.rounds,
                self.shares,
                members.join(", ")
            )
        }
    }

    /// The config of [`Pool`] answering every field with distinct answers.
    fn pool() -> Config {
        Config::new()
            .with("rounds", vec![3, 1])
            .with("shares", vec![0.5, 1.0])
            .with(
                "members",
                vec![
                    PluginConfig::of::<Matches>(),
                    PluginConfig::configured(Points::ID, points()),
                ],
            )
    }

    #[register]
    impl Configured for Ranker {
        fn summary(&self) -> String {
            let tie_breakers: Vec<_> = self
                .tie_breakers
                .iter()
                .map(|tie_breaker| tie_breaker.summary())
                .collect();
            format!(
                "ranker({}, [{}], {})",
                self.by.summary(),
                tie_breakers.join(", "),
                self.fallback
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), |fallback| fallback.summary())
            )
        }
    }

    /// The entry of the plugin `P`.
    fn entry_of<P: PluginType>() -> &'static PluginEntry {
        AnyAxis::of::<dyn Configured>()
            .entry(P::ID)
            .expect("the plugin is registered")
    }

    /// The summary of the plugin `P` as `config` configures it.
    fn built<P: PluginType>(config: Config) -> Result<String, ConfigError> {
        PluginConfig::configured(P::ID, config)
            .build::<dyn Configured>()
            .map(|plugin| plugin.summary())
    }

    /// The config of [`Points`] answering every field with its default.
    fn points() -> Config {
        Config::new()
            .with("for_win", 1.0)
            .with("rounds", 3)
            .with("counts_byes", true)
            .with("for_bye", 0.5)
    }

    /// The config of [`Ranker`] answering every field: by matches, without tie breakers and
    /// without a fallback.
    fn ranker() -> Config {
        Config::new()
            .with("by", PluginConfig::of::<Matches>())
            .with("tie_breakers", json!([]))
            .with("fallback", Value::Null)
    }

    #[test]
    fn a_field_taking_plugins_knows_the_axis_it_takes() {
        let schema = Ranker::schema();
        let kind = FieldKind::plugin::<dyn Configured>();

        assert_eq!(schema.field_named("by").map(Field::kind), Some(&kind));
        assert_eq!(
            schema.field_named("tie_breakers").map(Field::kind),
            Some(&FieldKind::list(kind.clone()))
        );
        assert_eq!(
            schema.field_named("fallback").map(Field::kind),
            Some(&FieldKind::optional(kind))
        );
        assert!(schema.field_named("losses").is_none());
        assert_eq!(
            schema.field_named("fallback").unwrap().kind().axis(),
            Some(AnyAxis::of::<dyn Configured>())
        );
        assert_eq!(
            entry_of::<Ranker>().sub_axes(),
            [AnyAxis::of::<dyn Configured>(); 3]
        );
        assert!(entry_of::<Points>().sub_axes().is_empty());
    }

    #[test]
    fn a_field_typed_as_a_set_takes_an_unordered_list() {
        let schema = Pool::schema();
        let kind = |name| schema.field_named(name).map(Field::kind);

        assert_eq!(kind("rounds"), Some(&FieldKind::set(FieldKind::Integer)));
        assert_eq!(kind("shares"), Some(&FieldKind::set(FieldKind::Decimal)));
        assert_eq!(
            kind("members"),
            Some(&FieldKind::set(FieldKind::plugin::<dyn Configured>()))
        );
        assert_ne!(
            FieldKind::set(FieldKind::Integer),
            FieldKind::list(FieldKind::Integer)
        );
        assert_eq!(
            entry_of::<Pool>().sub_axes(),
            [AnyAxis::of::<dyn Configured>()]
        );
    }

    #[test]
    fn a_set_of_distinct_answers_is_built() {
        assert_eq!(
            built::<Pool>(pool()).as_deref(),
            Ok("pool({1, 3}, Set([0.5, 1.0]), [matches, points(1, 3, true, 0.5)])")
        );
    }

    #[test]
    fn an_answer_repeated_in_a_set_is_an_error_at_its_second_position() {
        let duplicate = |config| match built::<Pool>(config) {
            Err(ConfigError::Duplicate { path }) => Some(path.to_string()),
            _ => None,
        };

        assert_eq!(
            duplicate(pool().with("rounds", vec![1, 2, 1])).as_deref(),
            Some("rounds[2]")
        );
        assert_eq!(
            duplicate(pool().with("shares", json!([1, 0.5, 1.0]))).as_deref(),
            Some("shares[2]")
        );
        // a plugin is the same plugin whatever it is configured with
        assert_eq!(
            duplicate(pool().with(
                "members",
                vec![
                    PluginConfig::configured(Points::ID, points()),
                    PluginConfig::configured(Points::ID, points().with("for_win", 2)),
                ],
            ))
            .as_deref(),
            Some("members[1]")
        );
    }

    #[test]
    fn a_plugin_repeated_in_a_list_is_an_error_even_if_the_config_is_incomplete() {
        let config = pool().with(
            "members",
            vec![
                PluginConfig::of::<Threshold>(),
                PluginConfig::of::<Threshold>(),
            ],
        );

        assert_eq!(
            Pool::schema().walk(&config, &Path::ROOT, false),
            Err(ConfigError::Duplicate {
                path: Path::of("members").index(1),
            })
        );
    }

    #[test]
    fn another_answer_repeated_in_a_list_is_no_error_unless_the_config_is_to_be_complete() {
        let config = Config::new()
            .with("least", 2)
            .with("weights", vec![1.0, 1.0]);

        assert!(
            Threshold::schema()
                .walk(&config, &Path::ROOT, false)
                .is_ok()
        );
        assert_eq!(
            Threshold::schema().walk(&config, &Path::ROOT, true),
            Err(ConfigError::Duplicate {
                path: Path::of("weights").index(1),
            })
        );
    }

    #[test]
    fn an_answer_repeated_in_an_ordered_list_is_an_error_too() {
        let config = Config::new().with("least", 2);

        assert_eq!(
            built::<Threshold>(config.clone().with("weights", vec![2.0, 1.0])).as_deref(),
            Ok("threshold(2, [2.0, 1.0])")
        );
        assert_eq!(
            built::<Threshold>(config.with("weights", json!([1, 2.0, 1.0]))),
            Err(ConfigError::Duplicate {
                path: Path::of("weights").index(2),
            })
        );
    }

    #[test]
    fn a_field_offering_a_default_still_has_to_be_answered() {
        assert_eq!(
            Points::schema()
                .field_named("rounds")
                .and_then(Field::default_value),
            Some(&json!(3))
        );
        assert_eq!(
            built::<Points>(Config::new()),
            Err(ConfigError::Unanswered {
                path: Path::of("for_win"),
            })
        );
        assert_eq!(
            built::<Points>(points()).as_deref(),
            Ok("points(1, 3, true, 0.5)")
        );
    }

    #[test]
    fn an_answer_is_kept_as_it_is_given() {
        let config = points()
            .with("for_win", 2.5)
            .with("counts_byes", false)
            .with("for_bye", Value::Null);

        assert_eq!(
            built::<Points>(config).as_deref(),
            Ok("points(2.5, 3, false, none)")
        );
    }

    #[test]
    fn a_whole_number_answers_a_decimal_field() {
        assert_eq!(
            built::<Points>(points().with("for_win", 2)).as_deref(),
            Ok("points(2, 3, true, 0.5)")
        );
    }

    #[test]
    fn a_field_without_a_default_has_to_be_answered() {
        assert_eq!(
            built::<Threshold>(Config::new().with("weights", json!([]))),
            Err(ConfigError::Unanswered {
                path: Path::of("least"),
            })
        );
        assert_eq!(
            built::<Threshold>(Config::new().with("least", 2).with("weights", json!([])))
                .as_deref(),
            Ok("threshold(2, [])")
        );
    }

    #[test]
    fn a_field_taking_a_list_is_answered_by_any_number_of_answers() {
        let config = Config::new()
            .with("least", 2)
            .with("weights", vec![1.0, 0.5]);

        assert_eq!(
            built::<Threshold>(config).as_deref(),
            Ok("threshold(2, [1.0, 0.5])")
        );
    }

    #[test]
    fn an_answer_to_a_field_the_schema_does_not_have_is_an_error() {
        assert_eq!(
            built::<Points>(Config::new().with("for_loss", 0.0)),
            Err(ConfigError::UnknownField {
                path: Path::of("for_loss"),
            })
        );
    }

    #[test]
    fn an_answer_of_another_kind_than_its_field_is_an_error() {
        assert_eq!(
            built::<Points>(points().with("counts_byes", 1)),
            Err(ConfigError::WrongKind {
                path: Path::of("counts_byes"),
                expected: "yes or no",
                found: "a whole number",
            })
        );
        assert_eq!(
            built::<Points>(points().with("rounds", 2.5)),
            Err(ConfigError::WrongKind {
                path: Path::of("rounds"),
                expected: "a whole number",
                found: "a number",
            })
        );
    }

    #[test]
    fn an_answer_of_another_kind_in_a_list_is_reported_by_its_position() {
        let config = Config::new()
            .with("least", 2)
            .with("weights", json!([1.0, true]));

        assert_eq!(
            built::<Threshold>(config),
            Err(ConfigError::WrongKind {
                path: Path::of("weights").index(1),
                expected: "a number",
                found: "yes or no",
            })
        );
    }

    #[test]
    fn an_answer_a_plugin_rejects_is_reported_as_its_field() {
        assert_eq!(
            built::<Points>(points().with("for_win", -1.0)),
            Err(ConfigError::Rejected {
                path: Path::of("for_win"),
                reason: "the points for a win cannot be negative".to_owned(),
            })
        );
    }

    #[test]
    fn a_field_taking_plugins_is_built_through_the_catalog() {
        let config = Config::new()
            .with(
                "by",
                PluginConfig::configured(Points::ID, points().with("for_win", 2.0)),
            )
            .with(
                "tie_breakers",
                vec![
                    PluginConfig::of::<Matches>(),
                    PluginConfig::of::<Threshold>()
                        .with("least", 4)
                        .with("weights", json!([])),
                ],
            )
            .with("fallback", PluginConfig::of::<Matches>());

        assert_eq!(
            built::<Ranker>(config).as_deref(),
            Ok("ranker(points(2, 3, true, 0.5), [matches, threshold(4, [])], matches)")
        );
    }

    #[test]
    fn a_plugin_field_offering_a_default_still_has_to_be_answered() {
        assert_eq!(
            built::<Ranker>(Config::new()),
            Err(ConfigError::Unanswered {
                path: Path::of("by"),
            })
        );
    }

    #[test]
    fn a_plugin_field_that_may_be_answered_with_nothing_takes_no_plugin() {
        assert_eq!(
            built::<Ranker>(ranker()).as_deref(),
            Ok("ranker(matches, [], none)")
        );
    }

    #[test]
    fn a_plugin_answering_a_field_that_takes_none_is_an_error() {
        let config = Config::new().with("by", 1);

        assert_eq!(
            built::<Ranker>(config),
            Err(ConfigError::WrongKind {
                path: Path::of("by"),
                expected: "one plugin",
                found: "a whole number",
            })
        );
    }

    #[test]
    fn an_error_in_a_nested_plugin_is_reported_by_the_path_of_its_field() {
        let config = ranker().with(
            "by",
            PluginConfig::configured(Points::ID, points().with("for_win", -1.0)),
        );

        assert_eq!(
            built::<Ranker>(config),
            Err(ConfigError::Rejected {
                path: Path::of("by").field("for_win"),
                reason: "the points for a win cannot be negative".to_owned(),
            })
        );
    }

    #[test]
    fn an_error_in_one_of_several_nested_plugins_is_reported_by_its_position() {
        let config = ranker().with(
            "tie_breakers",
            vec![
                PluginConfig::of::<Matches>(),
                PluginConfig::of::<Threshold>().with("weights", json!([])),
            ],
        );

        assert_eq!(
            built::<Ranker>(config),
            Err(ConfigError::Unanswered {
                path: Path::of("tie_breakers").index(1).field("least"),
            })
        );
    }

    #[test]
    fn an_error_in_a_nested_plugin_answering_an_optional_field_keeps_its_field() {
        let config = ranker().with(
            "fallback",
            PluginConfig::configured(Points::ID, points().with("for_win", -1.0)),
        );

        assert_eq!(
            built::<Ranker>(config),
            Err(ConfigError::Rejected {
                path: Path::of("fallback").field("for_win"),
                reason: "the points for a win cannot be negative".to_owned(),
            })
        );
    }

    #[test]
    fn a_plugin_not_registered_for_the_axis_cannot_answer_a_plugin_field() {
        let config = Config::new().with("by", PluginConfig::new("unregistered"));

        assert_eq!(
            built::<Ranker>(config),
            Err(ConfigError::UnknownPlugin {
                path: Path::of("by"),
            })
        );
    }

    #[test]
    fn a_built_plugin_serializes_as_the_walked_config_it_is_built_from() {
        let config = ranker()
            .with(
                "by",
                PluginConfig::configured(Points::ID, points().with("for_win", 2.0)),
            )
            .with("fallback", PluginConfig::of::<Matches>());
        let walked = Ranker::schema().walk(&config, &Path::ROOT, true).unwrap();
        let built = PluginConfig::configured(Ranker::ID, config)
            .build::<dyn Configured>()
            .unwrap();

        assert_eq!(
            walked.get("fallback"),
            Some(&json!({"type": "matches", "value": null}))
        );
        assert_eq!(
            serde_json::to_value(&built).ok(),
            Some(PluginConfig::configured(Ranker::ID, walked).into())
        );
    }

    #[test]
    fn a_plugin_without_parameters_is_walked_as_a_unit_struct_is_written() {
        let walked =
            |entry: &PluginEntry, config, complete| entry.walk(&config, &Path::ROOT, complete);

        assert_eq!(
            walked(entry_of::<Matches>(), Config::new(), true),
            Ok(Value::Null)
        );
        // an answer still being edited keeps its config an object
        assert_eq!(
            walked(entry_of::<Matches>(), Config::new(), false),
            Ok(json!({}))
        );
        assert_eq!(
            walked(
                entry_of::<Threshold>(),
                Config::new().with("least", 2).with("weights", json!([])),
                true
            ),
            Ok(json!({"least": 2, "weights": []}))
        );
    }

    #[test]
    fn an_unanswered_field_is_no_error_unless_the_config_is_to_be_complete() {
        let config = ranker().with(
            "tie_breakers",
            vec![PluginConfig::of::<Threshold>().with("weights", vec![1.0])],
        );

        assert_eq!(
            Ranker::schema()
                .walk(&config, &Path::ROOT, false)
                .map(Value::from),
            Ok(json!({
                "by": {"type": "matches", "value": {}},
                "tie_breakers": [{"type": "threshold", "value": {"weights": [1.0]}}],
                "fallback": null
            }))
        );
        assert_eq!(
            Ranker::schema().walk(&config, &Path::ROOT, true),
            Err(ConfigError::Unanswered {
                path: Path::of("tie_breakers").index(0).field("least"),
            })
        );
    }

    #[test]
    fn a_plugin_recommends_what_a_field_of_that_axis_offers() {
        let points = Points {
            for_win: 1.0,
            rounds: 3,
            counts_byes: true,
            for_bye: None,
        };

        assert_eq!(points.recommends(), vec![PluginConfig::of::<Matches>()]);
        assert!(Matches.recommends().is_empty());
    }
}
