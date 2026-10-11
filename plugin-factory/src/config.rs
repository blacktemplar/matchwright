//! The answers a plugin is configured with, i.e. what a program asking the questions of a
//! [`Schema`](crate::schema::Schema) collects.
//!
//! A [`Config`] holds one [`Value`] per field of a schema, keyed by the name of the field. An
//! answer is a scalar, a plugin, a list of answers of one kind or an answer that may be nothing,
//! so that a config is a tree as deep as the plugins and lists are nested. A plugin reads its
//! answers as the Rust types of its parameters, see [`Config::read`] and [`Answer`]; everything
//! that can go wrong while configuring is a [`ConfigError`] and carries the [`Path`] of the field
//! it is about.

use std::{
    any::TypeId,
    fmt::{self, Display, Formatter},
};

use thiserror::Error;

use crate::{
    catalog::PluginKind,
    schema::{Configurable, FieldKind},
};

/// The answer to one field of a schema.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// The answer to a [`FieldKind::Bool`] field.
    Bool(bool),
    /// The answer to a [`FieldKind::Integer`] field.
    Integer(i64),
    /// The answer to a [`FieldKind::Decimal`] field.
    Decimal(f64),
    /// The answer to a [`FieldKind::Plugin`] field.
    Plugin(PluginConfig),
    /// The answers to a [`FieldKind::List`] field, in the order they were given in.
    List(Vec<Value>),
    /// The answer to a [`FieldKind::Optional`] field, which may be nothing.
    Optional(Option<Box<Value>>),
}

impl Value {
    /// The answer of a [`FieldKind::Optional`] field that answers it with nothing.
    pub const NOTHING: Self = Self::Optional(None);

    /// The answer of a [`FieldKind::List`] field that answers it with no answer at all.
    pub const EMPTY_LIST: Self = Self::List(Vec::new());

    /// What this kind of answer is called in an error message, as in `FieldKind::name`.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "yes or no",
            Self::Integer(_) => "a whole number",
            Self::Decimal(_) => "a number",
            Self::Plugin(_) => "one plugin",
            Self::List(_) => "a list",
            Self::Optional(None) => "nothing",
            Self::Optional(Some(_)) => "an optional answer",
        }
    }
}

impl From<bool> for Value {
    fn from(answer: bool) -> Self {
        Self::Bool(answer)
    }
}

impl From<i64> for Value {
    fn from(answer: i64) -> Self {
        Self::Integer(answer)
    }
}

impl From<f64> for Value {
    fn from(answer: f64) -> Self {
        Self::Decimal(answer)
    }
}

impl From<PluginConfig> for Value {
    fn from(answer: PluginConfig) -> Self {
        Self::Plugin(answer)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(answers: Vec<T>) -> Self {
        Self::List(answers.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(answer: Option<T>) -> Self {
        Self::Optional(answer.map(|answer| Box::new(answer.into())))
    }
}

/// A Rust type one answer reads as, i.e. the type of a parameter a plugin is configured with.
///
/// Implemented for `bool`, `i64` and `f64`, for `Option<T>` of those, which reads a field that may
/// be answered with nothing, and for `Vec<T>`, which reads a field taking a list. The plugins
/// answering a field are read with [`Config::plugin`], [`Config::plugins`] and
/// [`Config::optional_plugin`] instead, since building one takes the kind it is of.
pub trait Answer: Sized {
    /// The kind of field this answers.
    fn kind() -> FieldKind;

    /// This from `answer`, or [`None`] if `answer` is not one a field of [`Self::kind`] takes.
    fn read(answer: &Value) -> Option<Self>;
}

impl Answer for bool {
    fn kind() -> FieldKind {
        FieldKind::Bool
    }

    fn read(answer: &Value) -> Option<Self> {
        match answer {
            Value::Bool(answer) => Some(*answer),
            _ => None,
        }
    }
}

impl Answer for i64 {
    fn kind() -> FieldKind {
        FieldKind::Integer
    }

    fn read(answer: &Value) -> Option<Self> {
        match answer {
            Value::Integer(answer) => Some(*answer),
            _ => None,
        }
    }
}

impl Answer for f64 {
    fn kind() -> FieldKind {
        FieldKind::Decimal
    }

    fn read(answer: &Value) -> Option<Self> {
        match answer {
            Value::Decimal(answer) => Some(*answer),
            // a whole number answers a decimal field
            Value::Integer(answer) => Some(*answer as Self),
            _ => None,
        }
    }
}

impl<T: Answer> Answer for Option<T> {
    fn kind() -> FieldKind {
        FieldKind::optional(T::kind())
    }

    fn read(answer: &Value) -> Option<Self> {
        match answer {
            Value::Optional(None) => Some(None),
            Value::Optional(Some(answer)) => T::read(answer).map(Some),
            _ => None,
        }
    }
}

impl<T: Answer> Answer for Vec<T> {
    fn kind() -> FieldKind {
        FieldKind::list(T::kind())
    }

    fn read(answer: &Value) -> Option<Self> {
        match answer {
            Value::List(answers) => answers.iter().map(T::read).collect(),
            _ => None,
        }
    }
}

/// One plugin as the answer to a field, i.e. which plugin it is and how it is configured.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginConfig {
    plugin: TypeId,
    config: Config,
}

impl PluginConfig {
    /// The plugin `P` configured by `config`.
    pub fn of<P: Configurable>(config: Config) -> Self {
        Self {
            plugin: TypeId::of::<P>(),
            config,
        }
    }

    /// The id of the plugin this configures, see [`PluginEntry::id`](crate::catalog::PluginEntry).
    pub fn plugin(&self) -> TypeId {
        self.plugin
    }

    /// The answers the plugin is configured with.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The plugin this configures, as a plugin of the kind `K`.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownPlugin`] if no plugin of kind `K` is registered with this id,
    /// and whatever configuring the plugin fails with otherwise. The path of the error addresses a
    /// field of this config, i.e. it is relative to the plugin.
    pub fn build<K: PluginKind>(&self) -> Result<Box<K::Object>, ConfigError> {
        let entry = K::entries()
            .find(|entry| entry.id() == self.plugin)
            .ok_or(ConfigError::UnknownPlugin { path: Path::ROOT })?;
        entry.build(&self.config)
    }
}

/// The answers one plugin is configured with, by the name of the field each answers.
///
/// A config that answers every field of a schema configures the plugin of that schema, see
/// [`Schema::complete`](crate::schema::Schema::complete); the default one answers nothing.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Config(Vec<(String, Value)>);

impl Config {
    /// A config answering nothing.
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// This config with `field` answered by `answer`, replacing an answer it holds already.
    #[must_use]
    pub fn with(mut self, field: impl Into<String>, answer: impl Into<Value>) -> Self {
        self.set(field, answer);
        self
    }

    /// Answers `field` with `answer`, replacing an answer this config holds already.
    pub fn set(&mut self, field: impl Into<String>, answer: impl Into<Value>) {
        let (field, answer) = (field.into(), answer.into());
        match self.0.iter_mut().find(|(name, _)| *name == field) {
            Some((_, answered)) => *answered = answer,
            None => self.0.push((field, answer)),
        }
    }

    /// Removes the answer to `field`, so that it is unanswered again.
    pub fn unset(&mut self, field: &str) {
        self.0.retain(|(name, _)| name != field);
    }

    /// The answer to `field`, or [`None`] if it is unanswered.
    pub fn get(&self, field: &str) -> Option<&Value> {
        self.0
            .iter()
            .find_map(|(name, answer)| (name == field).then_some(answer))
    }

    /// Every field this config answers with its answer, in the order they were answered in.
    pub fn answers(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(name, answer)| (name.as_str(), answer))
    }

    /// The answer to `field` as the Rust type `T` reads it, e.g. `f64` for a field taking a
    /// number, `Option<f64>` for one that may be answered with nothing and `Vec<f64>` for one
    /// taking a list of numbers.
    ///
    /// A field answered with nothing is answered; one with no answer at all is unanswered,
    /// whatever its kind.
    ///
    /// # Errors
    /// Returns [`ConfigError::Unanswered`] if the field is unanswered, and
    /// [`ConfigError::WrongKind`] if it is answered by something `T` does not read, anchored to
    /// where in a nested answer the mismatch is.
    pub fn read<T: Answer>(&self, field: &str) -> Result<T, ConfigError> {
        let answer = self.answer(field)?;
        T::read(answer).ok_or_else(|| {
            T::kind()
                .reject(answer)
                .unwrap_or_else(|| wrong_kind(&T::kind(), answer))
                .within(Segment::field(field))
        })
    }

    /// The plugin of kind `K` that answers `field`.
    ///
    /// # Errors
    /// Returns [`ConfigError::Unanswered`] if the field is unanswered,
    /// [`ConfigError::WrongKind`] if it is answered by something else, and whatever building the
    /// plugin fails with otherwise.
    pub fn plugin<K: PluginKind>(&self, field: &str) -> Result<Box<K::Object>, ConfigError> {
        plugin_from::<K>(self.answer(field)?).map_err(|error| error.within(Segment::field(field)))
    }

    /// The plugin of kind `K` that answers `field`, or [`None`] if it is answered with nothing.
    ///
    /// # Errors
    /// Returns [`ConfigError::Unanswered`] if the field is unanswered,
    /// [`ConfigError::WrongKind`] if it is answered by something else, and whatever building the
    /// plugin fails with otherwise.
    pub fn optional_plugin<K: PluginKind>(
        &self,
        field: &str,
    ) -> Result<Option<Box<K::Object>>, ConfigError> {
        let answer = self.answer(field)?;
        match answer {
            Value::Optional(None) => Ok(None),
            Value::Optional(Some(plugin)) => plugin_from::<K>(plugin).map(Some),
            _ => Err(wrong_kind(
                &FieldKind::optional(FieldKind::plugin::<K>()),
                answer,
            )),
        }
        .map_err(|error| error.within(Segment::field(field)))
    }

    /// The plugins of kind `K` that answer `field`, in the order they were given in.
    ///
    /// # Errors
    /// Returns [`ConfigError::Unanswered`] if the field is unanswered,
    /// [`ConfigError::WrongKind`] if it is answered by something else, and whatever building the
    /// first plugin that cannot be built fails with otherwise.
    pub fn plugins<K: PluginKind>(&self, field: &str) -> Result<Vec<Box<K::Object>>, ConfigError> {
        let answer = self.answer(field)?;
        match answer {
            Value::List(answers) => answers
                .iter()
                .enumerate()
                .map(|(position, answer)| {
                    plugin_from::<K>(answer).map_err(|error| error.within(Segment::Index(position)))
                })
                .collect(),
            _ => Err(wrong_kind(&FieldKind::plugins::<K>(), answer)),
        }
        .map_err(|error| error.within(Segment::field(field)))
    }

    /// The answer to `field`, whatever kind it is of.
    fn answer(&self, field: &str) -> Result<&Value, ConfigError> {
        self.get(field).ok_or_else(|| ConfigError::Unanswered {
            path: Path::of(field),
        })
    }
}

/// The plugin of kind `K` that `answer` configures, with the paths of its errors relative to the
/// field `answer` is the answer to.
fn plugin_from<K: PluginKind>(answer: &Value) -> Result<Box<K::Object>, ConfigError> {
    match answer {
        Value::Plugin(plugin) => plugin.build::<K>(),
        _ => Err(wrong_kind(&FieldKind::plugin::<K>(), answer)),
    }
}

/// The error of a field taking an answer of kind `expected` being answered by `found`, as the
/// config holding that field sees it, i.e. to be prefixed by the field.
fn wrong_kind(expected: &FieldKind, found: &Value) -> ConfigError {
    ConfigError::WrongKind {
        path: Path::ROOT,
        expected: expected.name(),
        found: found.name(),
    }
}

/// Where in a config something is: the field it is about, prefixed by the fields and positions of
/// the plugins and lists the field belongs to.
///
/// [`Display`] writes a path as `metrics[1].points_for_win`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Path(Vec<Segment>);

impl Path {
    /// The config itself rather than a field of it.
    pub const ROOT: Self = Self(Vec::new());

    /// The path of the field `field` of a config.
    pub fn of(field: impl Into<String>) -> Self {
        Self::ROOT.then(Segment::field(field))
    }

    /// This path followed by `segment`, i.e. one level further in.
    #[must_use]
    pub fn then(mut self, segment: Segment) -> Self {
        self.0.push(segment);
        self
    }

    /// The segments of this path, from the outermost field inwards.
    pub fn segments(&self) -> &[Segment] {
        &self.0
    }

    /// Prefixes this path by `segment`, i.e. makes it the path of the same thing seen from one
    /// config further out.
    fn prepend(&mut self, segment: Segment) {
        self.0.insert(0, segment);
    }
}

impl Display for Path {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return formatter.write_str("the configuration");
        }
        for (position, segment) in self.0.iter().enumerate() {
            match segment {
                Segment::Field(field) if position == 0 => write!(formatter, "{field}")?,
                Segment::Field(field) => write!(formatter, ".{field}")?,
                Segment::Index(index) => write!(formatter, "[{index}]")?,
            }
        }
        Ok(())
    }
}

/// One step of a [`Path`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// A field, by name.
    Field(String),
    /// One answer of a field taking a list, by position.
    Index(usize),
}

impl Segment {
    /// The segment of the field `field`.
    pub fn field(field: impl Into<String>) -> Self {
        Self::Field(field.into())
    }
}

/// Why a config does not configure the plugin it is given to.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{path} is not a field of the schema")]
    UnknownField { path: Path },
    #[error("{path} is unanswered and has no default")]
    Unanswered { path: Path },
    #[error("{path} is {found}, but it takes {expected}")]
    WrongKind {
        path: Path,
        expected: &'static str,
        found: &'static str,
    },
    #[error("the plugin answering {path} is not registered for the kind of the field")]
    UnknownPlugin { path: Path },
    #[error("{path} is rejected: {reason}")]
    Rejected { path: Path, reason: String },
}

impl ConfigError {
    /// The error of a plugin rejecting the answer to its field `field` because of `reason`, e.g.
    /// "the points for a win cannot be negative".
    pub fn rejected(field: &str, reason: impl Into<String>) -> Self {
        Self::Rejected {
            path: Path::of(field),
            reason: reason.into(),
        }
    }

    /// The error of a plugin rejecting its configuration as a whole because of `reason`, i.e. one
    /// that belongs to no single field.
    pub fn rejected_config(reason: impl Into<String>) -> Self {
        Self::Rejected {
            path: Path::ROOT,
            reason: reason.into(),
        }
    }

    /// What this error is about.
    pub fn path(&self) -> &Path {
        match self {
            Self::UnknownField { path }
            | Self::Unanswered { path }
            | Self::WrongKind { path, .. }
            | Self::UnknownPlugin { path }
            | Self::Rejected { path, .. } => path,
        }
    }

    /// This error as the config one level further out sees it, i.e. with `segment` in front of its
    /// path.
    #[must_use]
    pub fn within(mut self, segment: Segment) -> Self {
        match &mut self {
            Self::UnknownField { path }
            | Self::Unanswered { path }
            | Self::WrongKind { path, .. }
            | Self::UnknownPlugin { path }
            | Self::Rejected { path, .. } => path.prepend(segment),
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested() -> Path {
        Path::of("metrics")
            .then(Segment::Index(1))
            .then(Segment::field("points_for_win"))
    }

    #[test]
    fn a_path_reads_as_the_field_it_addresses() {
        assert_eq!(nested().to_string(), "metrics[1].points_for_win");
        assert_eq!(Path::of("ranker").to_string(), "ranker");
        assert_eq!(Path::ROOT.to_string(), "the configuration");
    }

    #[test]
    fn an_error_seen_from_further_out_is_prefixed_by_the_field_it_is_in() {
        let error = ConfigError::rejected("points_for_win", "cannot be negative")
            .within(Segment::Index(1))
            .within(Segment::field("metrics"));

        assert_eq!(error.path(), &nested());
        assert_eq!(
            error.to_string(),
            "metrics[1].points_for_win is rejected: cannot be negative"
        );
    }

    #[test]
    fn an_unknown_plugin_is_reported_as_the_field_it_answers() {
        let error =
            ConfigError::UnknownPlugin { path: Path::ROOT }.within(Segment::field("ranker"));

        assert_eq!(error.path(), &Path::of("ranker"));
    }

    #[test]
    fn an_answer_replaces_the_one_of_the_same_field() {
        let config = Config::new()
            .with("points_for_win", 1.0)
            .with("rounds", 3i64)
            .with("points_for_win", 2.0);

        assert_eq!(config.read("points_for_win"), Ok(2.0));
        assert_eq!(
            config.answers().map(|(field, _)| field).collect::<Vec<_>>(),
            ["points_for_win", "rounds"]
        );
    }

    #[test]
    fn an_unset_field_is_unanswered() {
        let mut config = Config::new().with("third_place_match", true);
        config.unset("third_place_match");

        assert_eq!(
            config.read::<bool>("third_place_match"),
            Err(ConfigError::Unanswered {
                path: Path::of("third_place_match"),
            })
        );
    }

    #[test]
    fn an_answer_of_another_kind_is_an_error() {
        let config = Config::new().with("rounds", true);

        assert_eq!(
            config.read::<i64>("rounds"),
            Err(ConfigError::WrongKind {
                path: Path::of("rounds"),
                expected: "a whole number",
                found: "yes or no",
            })
        );
    }

    #[test]
    fn a_whole_number_answers_a_decimal_field() {
        let config = Config::new().with("points_for_win", 1i64);

        assert_eq!(config.read("points_for_win"), Ok(1.0));
    }

    #[test]
    fn a_field_that_may_be_answered_with_nothing_reads_as_an_option() {
        let config = Config::new()
            .with("points_for_bye", Some(0.5))
            .with("points_for_loss", Value::NOTHING);

        assert_eq!(config.read("points_for_bye"), Ok(Some(0.5)));
        assert_eq!(config.read::<Option<f64>>("points_for_loss"), Ok(None));
    }

    #[test]
    fn an_answer_that_is_not_optional_does_not_answer_an_optional_field() {
        let config = Config::new().with("points_for_bye", 0.5);

        assert_eq!(
            config.read::<Option<f64>>("points_for_bye"),
            Err(ConfigError::WrongKind {
                path: Path::of("points_for_bye"),
                expected: "an optional answer",
                found: "a number",
            })
        );
    }

    #[test]
    fn a_field_taking_a_list_reads_as_a_vec() {
        let config = Config::new().with("weights", vec![1.0, 0.5]);

        assert_eq!(config.read("weights"), Ok(vec![1.0, 0.5]));
        assert_eq!(
            config
                .read::<Vec<f64>>("weights")
                .map(|weights| weights.len()),
            Ok(2)
        );
    }

    #[test]
    fn an_answer_of_another_kind_in_a_list_is_reported_by_its_position() {
        let config = Config::new().with("weights", vec![Value::Decimal(1.0), Value::Bool(true)]);

        assert_eq!(
            config.read::<Vec<f64>>("weights"),
            Err(ConfigError::WrongKind {
                path: Path::of("weights").then(Segment::Index(1)),
                expected: "a number",
                found: "yes or no",
            })
        );
    }

    #[test]
    fn a_list_and_an_optional_answer_nest() {
        let config = Config::new().with("rounds_per_phase", vec![Some(3i64), None]);

        assert_eq!(
            config.read("rounds_per_phase"),
            Ok(vec![Some(3i64), None::<i64>])
        );
    }
}
