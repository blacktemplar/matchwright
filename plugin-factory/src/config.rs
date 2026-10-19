//! The answers a plugin is configured with, i.e. what a program asking the questions of a
//! [`Schema`](crate::schema::Schema) collects.
//!
//! A [`Config`] holds one JSON [`Value`] per field of a schema, keyed by the name of the field. An
//! answer is a scalar, a plugin, see [`PluginConfig`], a list of answers of one kind or an answer
//! that may be nothing, so that a config is a tree as deep as the plugins and lists are nested. A
//! plugin is built from its config by deserializing it, so it reads its answers through its own
//! `Deserialize` impl; everything that can go wrong while configuring is a [`ConfigError`] and
//! carries the [`Path`] of the field it is about.

use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::Map;
pub use serde_json::Value;
use thiserror::Error;

use crate::{
    PluginType,
    catalog::{AnyAxis, Axis},
    schema::Configurable,
};

/// One plugin as the answer to a field, i.e. which plugin it is and how it is configured.
///
/// As a [`Value`] it is the object `{"type": id, "value": config}`, which is how `typetag`
/// writes a plugin tagged adjacently with `tag = "type", content = "value"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginConfig {
    #[serde(rename = "type")]
    plugin: String,
    #[serde(rename = "value")]
    config: Config,
}

impl PluginConfig {
    /// The plugin `P` with nothing answered.
    pub fn of<P: PluginType>() -> Self {
        Self::new(P::ID)
    }

    /// The plugin with the id `plugin`, see [`PluginType::ID`], with nothing answered.
    pub fn new(plugin: impl Into<String>) -> Self {
        Self::configured(plugin, Config::new())
    }

    /// The plugin with the id `plugin` configured by `config`.
    pub(crate) fn configured(plugin: impl Into<String>, config: Config) -> Self {
        Self {
            plugin: plugin.into(),
            config,
        }
    }

    /// This plugin with `field` answered by `answer`, replacing an answer it holds already, see
    /// [`Config::with`].
    #[must_use]
    pub fn with(mut self, field: impl Into<String>, answer: impl Into<Value>) -> Self {
        self.config.set(field, answer);
        self
    }

    /// The id of the plugin this configures, see [`PluginType::ID`].
    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    /// The answers the plugin is configured with.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The plugin this configures, as a plugin of the axis `A`, e.g. `build::<dyn Metric>()`.
    /// Every field of its config, and of the configs of the plugins answering its fields, has to
    /// be answered; defaults are not filled in.
    ///
    /// The plugin is deserialized from the answer through the `Deserialize` impl of `Box<A>`,
    /// i.e. the way a saved plugin is loaded, which knows the plugin by its id, see
    /// [`register`](crate::register).
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownPlugin`] if no plugin of the axis `A` is registered with
    /// this id, [`ConfigError::Unanswered`] if a field is unanswered, [`ConfigError::Rejected`]
    /// if deserializing the answer fails, and whatever checking the config against the schemas
    /// fails with otherwise. The path of the error addresses a field of this config, i.e. it is
    /// relative to the plugin.
    pub fn build<A: Axis + ?Sized>(&self) -> Result<Box<A>, ConfigError>
    where
        Box<A>: DeserializeOwned,
    {
        let answer = AnyAxis::of::<A>().walk(self, &Path::ROOT, true)?;
        deserialize_plugin(&answer, &Path::ROOT)
    }

    /// The plugin configured by `answer`, or [`None`] if `answer` is not a plugin.
    ///
    /// # Examples
    /// ```
    /// use plugin_factory::config::{PluginConfig, Value};
    ///
    /// let answer = Value::from(PluginConfig::new("points").with("points_for_win", 3));
    ///
    /// assert_eq!(
    ///     PluginConfig::from_answer(&answer),
    ///     Some(PluginConfig::new("points").with("points_for_win", 3))
    /// );
    /// assert_eq!(PluginConfig::from_answer(&Value::from(3)), None);
    /// ```
    pub fn from_answer(answer: &Value) -> Option<Self> {
        Self::deserialize(answer).ok()
    }
}

impl From<PluginConfig> for Value {
    fn from(answer: PluginConfig) -> Self {
        let PluginConfig { plugin, config } = answer;
        plugin_answer(plugin, config.into())
    }
}

/// The answer naming the plugin with the id `plugin` and holding `config`, as [`PluginConfig`]
/// writes it.
pub(crate) fn plugin_answer(plugin: String, config: Value) -> Value {
    Value::Object(Map::from_iter([
        ("type".to_owned(), Value::String(plugin)),
        ("value".to_owned(), config),
    ]))
}

/// The answers one plugin is configured with, by the name of the field each answers.
///
/// A config that answers every field of a schema configures the plugin of that schema; the
/// default one answers nothing. As a [`Value`] it is the object of those answers; `null`
/// deserializes as the config answering nothing, which is how the config of a unit struct is
/// written, see [`Configurable`].
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Config(pub(crate) Map<String, Value>);

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::deserialize(deserializer).map(|answers| Self(answers.unwrap_or_default()))
    }
}

impl From<Config> for Value {
    fn from(config: Config) -> Self {
        Self::Object(config.0)
    }
}

impl Config {
    /// A config answering nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// This config with `field` answered by `answer`, replacing an answer it holds already.
    #[must_use]
    pub fn with(mut self, field: impl Into<String>, answer: impl Into<Value>) -> Self {
        self.set(field, answer);
        self
    }

    /// Answers `field` with `answer`, replacing an answer this config holds already.
    pub(crate) fn set(&mut self, field: impl Into<String>, answer: impl Into<Value>) {
        self.0.insert(field.into(), answer.into());
    }

    /// The answer to `field`, or [`None`] if it is unanswered.
    ///
    /// A field answered with nothing, i.e. with [`Value::Null`], is answered.
    pub fn get(&self, field: &str) -> Option<&Value> {
        self.0.get(field)
    }

    /// The answer at `path`, or [`None`] if it is unanswered or `path` addresses nothing in this
    /// config, e.g. the root or a field of a plugin that is not chosen.
    ///
    /// # Examples
    /// ```
    /// use plugin_factory::config::{Config, PluginConfig, Value};
    ///
    /// let config = Config::new().with(
    ///     "metrics",
    ///     vec![Value::from(
    ///         PluginConfig::new("points").with("points_for_win", 3),
    ///     )],
    /// );
    ///
    /// assert_eq!(
    ///     config.get_at(&"metrics[0].points_for_win".parse().unwrap()),
    ///     Some(&Value::from(3))
    /// );
    /// assert_eq!(config.get_at(&"metrics[1]".parse().unwrap()), None);
    /// ```
    pub fn get_at(&self, path: &Path) -> Option<&Value> {
        let (Segment::Field(name), rest) = path.segments().split_first()? else {
            return None;
        };
        rest.iter()
            .try_fold(self.0.get(name)?, |answer, segment| match segment {
                // the config of a plugin is its answer's "value"
                Segment::Field(name) => answer.get("value")?.get(name),
                Segment::Index(index) => answer.get(index),
            })
    }

    /// [`Self::get_at`] for the path made of `segments`, mutably.
    pub(crate) fn get_at_mut(&mut self, segments: &[Segment]) -> Option<&mut Value> {
        let (Segment::Field(name), rest) = segments.split_first()? else {
            return None;
        };
        rest.iter()
            .try_fold(self.0.get_mut(name)?, |answer, segment| match segment {
                Segment::Field(name) => answer.get_mut("value")?.get_mut(name),
                Segment::Index(index) => answer.get_mut(index),
            })
    }

    /// Every field this config answers with its answer, in no particular order.
    pub(crate) fn answers(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(name, answer)| (name.as_str(), answer))
    }
}

/// `walked`, a config walked as complete, as the value a [`Configurable`] is deserialized from:
/// `null` if it is a unit struct, which is how serde writes one.
pub(crate) fn complete<T: Configurable>(walked: Config) -> Value {
    if T::IS_UNIT {
        Value::Null
    } else {
        walked.into()
    }
}

/// `T` deserialized from `config`, a complete config at `at` as [`complete`] writes it.
///
/// # Errors
/// Returns [`ConfigError::Rejected`] with the path of the answer `T` rejects.
pub(crate) fn deserialize<T: DeserializeOwned>(
    config: &Value,
    at: &Path,
) -> Result<T, ConfigError> {
    deserialize_at(config, at, true)
}

/// The plugin `P` deserialized from `answer`, a complete plugin answer at `at` as
/// [`AnyAxis::walk`] writes it, i.e. still tagged with its id.
///
/// # Errors
/// Returns [`ConfigError::Rejected`] with the path of the answer `P` rejects.
fn deserialize_plugin<P: DeserializeOwned>(answer: &Value, at: &Path) -> Result<P, ConfigError> {
    deserialize_at(answer, at, false)
}

/// `T` deserialized from `value` at `at`, which is a config if `is_config` and a plugin answer
/// otherwise.
fn deserialize_at<T: DeserializeOwned>(
    value: &Value,
    at: &Path,
    is_config: bool,
) -> Result<T, ConfigError> {
    serde_path_to_error::deserialize(value).map_err(|error| ConfigError::Rejected {
        path: at.clone().deserialized(error.path(), is_config),
        reason: error.inner().to_string(),
    })
}

/// What an error message calls an answer of each kind, both one that is given, see [`name_of`],
/// and one a field takes, see [`FieldKind`](crate::schema::FieldKind).
pub(crate) mod names {
    pub(crate) const BOOL: &str = "yes or no";
    pub(crate) const INTEGER: &str = "a whole number";
    pub(crate) const DECIMAL: &str = "a number";
    pub(crate) const PLUGIN: &str = "one plugin";
    pub(crate) const LIST: &str = "a list";
}

/// What `answer` is called in an error message, as the kind of answer it is.
pub(crate) fn name_of(answer: &Value) -> &'static str {
    match answer {
        Value::Null => "nothing",
        Value::Bool(_) => names::BOOL,
        Value::Number(number) if number.is_i64() => names::INTEGER,
        Value::Number(_) => names::DECIMAL,
        Value::String(_) => "text",
        Value::Array(_) => names::LIST,
        Value::Object(_) if PluginConfig::from_answer(answer).is_some() => names::PLUGIN,
        Value::Object(_) => "an object",
    }
}

/// Where in a config something is: the field it is about, prefixed by the fields and positions of
/// the plugins and lists the field belongs to.
///
/// [`Display`] writes a path as `metrics[1].points_for_win`, and [`FromStr`] reads it back, e.g.
/// `"metrics[1].points_for_win".parse()`. [`Self::ROOT`] is written as "the configuration" and
/// not read back.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Path(Vec<Segment>);

impl Path {
    /// The config itself rather than a field of it.
    pub const ROOT: Self = Self(Vec::new());

    /// This path followed by the field `name`: a field of the config this path addresses, or of
    /// the plugin answering what it addresses.
    #[must_use]
    pub fn field(self, name: impl Into<String>) -> Self {
        self.then(Segment::Field(name.into()))
    }

    /// This path followed by the position `position` in the list it addresses.
    #[must_use]
    pub fn index(self, position: usize) -> Self {
        self.then(Segment::Index(position))
    }

    /// The segments of this path, from the outermost field inwards.
    pub fn segments(&self) -> &[Segment] {
        &self.0
    }

    /// The path of the field `field` of a config.
    #[cfg(test)]
    pub(crate) fn of(field: impl Into<String>) -> Self {
        Self::ROOT.field(field)
    }

    /// The path of what this path is within, or [`None`] for the root.
    pub(crate) fn parent(&self) -> Option<Self> {
        let (_, parent) = self.0.split_last()?;
        Some(Self(parent.to_vec()))
    }

    /// This path followed by `segment`, i.e. one level further in.
    fn then(mut self, segment: Segment) -> Self {
        self.0.push(segment);
        self
    }

    /// Whether this path is `prefix` or addresses something within what `prefix` addresses.
    pub(crate) fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(&prefix.0)
    }

    /// This path followed by where deserializing the config it addresses, or the plugin answer
    /// unless `is_config`, failed, where `deserialized` is where in it as a [`Value`] that is.
    fn deserialized(self, deserialized: &serde_path_to_error::Path, is_config: bool) -> Self {
        use serde_path_to_error::Segment as Deserialized;

        // a config holds answers, a list holds answers, and a plugin answer holds its config under
        // "value"; its "type" is left out like anything not addressable as a field
        let mut path = self;
        let mut in_config = is_config;
        for segment in deserialized.iter() {
            match segment {
                Deserialized::Map { key } if in_config => {
                    path = path.field(key.as_str());
                    in_config = false;
                }
                Deserialized::Map { key } if key == "value" => in_config = true,
                Deserialized::Seq { index } if !in_config => path = path.index(*index),
                _ => break,
            }
        }
        path
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

impl FromStr for Path {
    type Err = ParsePathError;

    /// The path [`Display`] writes as `text`: fields separated by `.`, each followed by any
    /// number of positions in brackets, e.g. `metrics[1].points_for_win`.
    ///
    /// # Errors
    /// Returns [`ParsePathError`] if `text` is not such a path, e.g. if it is empty, or a field
    /// name is empty or contains `[` or `]`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || ParsePathError(text.to_owned());
        let mut path = Self::ROOT;
        for part in text.split('.') {
            let (name, mut positions) = part.split_at(part.find('[').unwrap_or(part.len()));
            if name.is_empty() || name.contains(']') {
                return Err(error());
            }
            path = path.field(name);
            while let Some(rest) = positions.strip_prefix('[') {
                let (position, rest) = rest.split_once(']').ok_or_else(error)?;
                path = path.index(position.parse().map_err(|_| error())?);
                positions = rest;
            }
            if !positions.is_empty() {
                return Err(error());
            }
        }
        Ok(path)
    }
}

/// Why text is not a [`Path`] written as [`Display`] writes one.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("{0:?} is not a path")]
pub struct ParsePathError(String);

/// One step of a [`Path`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// A field, by name.
    Field(String),
    /// One answer of a field taking a list, by position.
    Index(usize),
}

/// Why a config does not configure the plugin it is given to.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{path} is not a field of the schema")]
    UnknownField { path: Path },
    #[error("{path} is unanswered")]
    Unanswered { path: Path },
    #[error("{path} is {found}, but it takes {expected}")]
    WrongKind {
        path: Path,
        expected: &'static str,
        found: &'static str,
    },
    #[error("the plugin answering {path} is not registered for the axis of the field")]
    UnknownPlugin { path: Path },
    /// An answer in a list is the same as one before it.
    #[error("{path} repeats an earlier answer, but the list takes each answer only once")]
    Duplicate { path: Path },
    /// The plugin's `Deserialize` impl rejected the answer, e.g. through a custom error.
    #[error("{path} is rejected: {reason}")]
    Rejected { path: Path, reason: String },
}

impl ConfigError {
    /// What this error is about.
    pub fn path(&self) -> &Path {
        match self {
            Self::UnknownField { path }
            | Self::Unanswered { path }
            | Self::WrongKind { path, .. }
            | Self::UnknownPlugin { path }
            | Self::Duplicate { path }
            | Self::Rejected { path, .. } => path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested() -> Path {
        Path::ROOT.field("metrics").index(1).field("points_for_win")
    }

    #[test]
    fn a_path_reads_as_the_field_it_addresses() {
        assert_eq!(nested().to_string(), "metrics[1].points_for_win");
        assert_eq!(Path::of("ranker").to_string(), "ranker");
        assert_eq!(Path::ROOT.to_string(), "the configuration");
    }

    #[test]
    fn a_path_is_read_back_as_it_is_written() {
        let paths = [
            nested(),
            Path::of("ranker"),
            Path::of("tie_breakers").index(0).index(2).field("by"),
        ];

        for path in paths {
            assert_eq!(path.to_string().parse(), Ok(path));
        }
    }

    #[test]
    fn text_that_is_not_a_path_is_an_error() {
        for text in [
            "",
            "[1]",
            "metrics.",
            "metrics[",
            "metrics[x]",
            "metrics[1]x",
            "metrics]",
        ] {
            assert_eq!(
                text.parse::<Path>(),
                Err(ParsePathError(text.to_owned())),
                "{text}"
            );
        }
    }

    #[test]
    fn an_error_reads_as_the_path_it_is_about() {
        let error = ConfigError::Rejected {
            path: nested(),
            reason: "cannot be negative".to_owned(),
        };

        assert_eq!(error.path(), &nested());
        assert_eq!(
            error.to_string(),
            "metrics[1].points_for_win is rejected: cannot be negative"
        );
    }

    #[test]
    fn an_answer_replaces_the_one_of_the_same_field() {
        let config = Config::new()
            .with("points_for_win", 1.0)
            .with("rounds", 3)
            .with("points_for_win", 2.0);

        assert_eq!(config.get("points_for_win"), Some(&Value::from(2.0)));
        assert_eq!(config.answers().count(), 2);
    }

    #[test]
    fn a_field_answered_with_nothing_is_answered() {
        let config = Config::new().with("points_for_bye", Value::Null);

        assert_eq!(config.get("points_for_bye"), Some(&Value::Null));
    }

    #[test]
    fn a_plugin_answer_is_written_as_typetag_writes_an_adjacently_tagged_plugin() {
        let answer = PluginConfig::new("points").with("for_win", 2.0);

        assert_eq!(
            Value::from(answer.clone()),
            serde_json::json!({"type": "points", "value": {"for_win": 2.0}})
        );
        assert_eq!(
            serde_json::to_value(&answer).ok(),
            Some(Value::from(answer.clone()))
        );
        assert_eq!(
            PluginConfig::from_answer(&Value::from(answer.clone())),
            Some(answer)
        );
    }

    /// A plugin with a field rejecting a negative answer.
    #[derive(Debug, Deserialize)]
    struct Points {
        #[serde(rename = "for_win")]
        _for_win: u32,
    }

    /// A plugin answer as typetag writes it, of which only the config is read.
    #[derive(Debug, Deserialize)]
    struct Tagged {
        #[serde(rename = "value")]
        _value: Points,
    }

    /// A plugin made of plugins.
    #[derive(Debug, Deserialize)]
    struct Ranker {
        #[serde(rename = "tie_breakers")]
        _tie_breakers: Vec<Tagged>,
    }

    #[test]
    fn the_path_of_a_deserialization_error_leaves_out_how_plugins_are_written() {
        let config = serde_json::json!({
            "tie_breakers": [{"type": "points", "value": {"for_win": -1.0}}]
        });

        assert_eq!(
            deserialize::<Ranker>(&config, &Path::ROOT)
                .unwrap_err()
                .path(),
            &Path::of("tie_breakers").index(0).field("for_win")
        );
    }

    #[test]
    fn the_path_of_a_deserialization_error_follows_the_path_of_the_plugin_answer() {
        let answer = serde_json::json!({"type": "points", "value": {"for_win": -1.0}});
        let rejected_at = |at: &Path| {
            deserialize_plugin::<Tagged>(&answer, at)
                .unwrap_err()
                .path()
                .clone()
        };

        assert_eq!(rejected_at(&Path::ROOT), Path::of("for_win"));
        assert_eq!(
            rejected_at(&Path::of("metrics").index(1)),
            Path::of("metrics").index(1).field("for_win")
        );
    }
}
