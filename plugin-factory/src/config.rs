//! The answers a plugin is configured with, i.e. what a program asking the questions of a
//! [`Schema`](crate::schema::Schema) collects.
//!
//! A [`Config`] holds one JSON [`Value`] per field of a schema, keyed by the name of the field. An
//! answer is a scalar, a plugin, see [`PluginConfig`], a list of answers of one kind or an answer
//! that may be nothing, so that a config is a tree as deep as the plugins and lists are nested. A
//! plugin is built from its config by deserializing it, so it reads its answers through its own
//! `Deserialize` impl; everything that can go wrong while configuring is a [`ConfigError`] and
//! carries the [`Path`] of the field it is about.

use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::Map;
pub use serde_json::Value;
use thiserror::Error;

use crate::{
    PluginType,
    catalog::{Kind, PluginKind},
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
    /// The plugin `P` configured by `config`.
    pub fn of<P: PluginType>(config: Config) -> Self {
        Self::new(P::ID, config)
    }

    /// The plugin with the id `plugin` configured by `config`, see [`PluginType::ID`].
    pub fn new(plugin: impl Into<String>, config: Config) -> Self {
        Self {
            plugin: plugin.into(),
            config,
        }
    }

    /// The id of the plugin this configures, see [`PluginType::ID`].
    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    /// The answers the plugin is configured with.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The plugin this configures, as a plugin of the kind `K`, where every field its config
    /// leaves unanswered is answered by the default of the field, and likewise for the plugins
    /// answering its fields, see [`Schema::complete`](crate::schema::Schema::complete).
    ///
    /// The plugin is deserialized from the completed answer through the `Deserialize` impl of
    /// `Box<K::Object>`, i.e. the way a saved plugin is loaded, so that impl has to know the
    /// plugin by its id, see [`check_serialization`](crate::catalog::check_serialization).
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownPlugin`] if no plugin of kind `K` is registered with this id,
    /// [`ConfigError::Unanswered`] if a field without a default is unanswered,
    /// [`ConfigError::Rejected`] if deserializing the completed answer fails, and whatever
    /// completing the config fails with otherwise. The path of the error addresses a field of
    /// this config, i.e. it is relative to the plugin.
    pub fn build<K: PluginKind>(&self) -> Result<Box<K::Object>, ConfigError>
    where
        Box<K::Object>: DeserializeOwned,
    {
        deserialize_plugin(&Kind::of::<K>().complete(self)?)
    }

    /// The plugin configured by `answer`, or [`None`] if `answer` is not a plugin.
    pub(crate) fn from_answer(answer: &Value) -> Option<Self> {
        Self::deserialize(answer).ok()
    }
}

impl From<PluginConfig> for Value {
    fn from(answer: PluginConfig) -> Self {
        let PluginConfig { plugin, config } = answer;
        Self::Object(Map::from_iter([
            ("type".to_owned(), Self::String(plugin)),
            ("value".to_owned(), Self::Object(config.0)),
        ]))
    }
}

/// The answers one plugin is configured with, by the name of the field each answers.
///
/// A config that answers every field of a schema configures the plugin of that schema, see
/// [`Schema::complete`](crate::schema::Schema::complete); the default one answers nothing. As a
/// [`Value`] it is the object of those answers; `null` deserializes as the config answering
/// nothing, which is how the config of a unit struct is written, see
/// [`Configurable`](crate::schema::Configurable).
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
    pub fn set(&mut self, field: impl Into<String>, answer: impl Into<Value>) {
        self.0.insert(field.into(), answer.into());
    }

    /// Removes the answer to `field`, so that it is unanswered again.
    pub fn unset(&mut self, field: &str) {
        self.0.remove(field);
    }

    /// The answer to `field`, or [`None`] if it is unanswered.
    ///
    /// A field answered with nothing, i.e. with [`Value::Null`], is answered.
    pub fn get(&self, field: &str) -> Option<&Value> {
        self.0.get(field)
    }

    /// Every field this config answers with its answer, in no particular order.
    pub fn answers(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(name, answer)| (name.as_str(), answer))
    }
}

/// The plugin `P` deserialized from `config`, a completed config as
/// [`PluginEntry::complete`](crate::catalog::PluginEntry::complete) writes it.
///
/// # Errors
/// Returns [`ConfigError::Rejected`] with the path of the answer `P` rejects.
pub(crate) fn deserialize<P: DeserializeOwned>(config: &Value) -> Result<P, ConfigError> {
    deserialize_at(config, true)
}

/// The plugin `P` deserialized from `answer`, a completed plugin answer as
/// [`Kind::complete`] writes it, i.e. still tagged with its id.
///
/// # Errors
/// Returns [`ConfigError::Rejected`] with the path of the answer `P` rejects.
fn deserialize_plugin<P: DeserializeOwned>(answer: &Value) -> Result<P, ConfigError> {
    deserialize_at(answer, false)
}

/// `T` deserialized from `value`, which is a config if `is_config` and a plugin answer otherwise.
fn deserialize_at<T: DeserializeOwned>(value: &Value, is_config: bool) -> Result<T, ConfigError> {
    serde_path_to_error::deserialize(value).map_err(|error| ConfigError::Rejected {
        path: Path::of_deserialized(error.path(), is_config),
        reason: error.inner().to_string(),
    })
}

/// Whether `P` deserializes from `null`, i.e. is a unit struct, whose config is then written as
/// `null` rather than as an empty object.
pub(crate) fn deserializes_from_null<P: DeserializeOwned>() -> bool {
    serde_json::from_value::<P>(Value::Null).is_ok()
}

/// What `answer` is called in an error message, as the kind of answer it is.
pub(crate) fn name_of(answer: &Value) -> &'static str {
    match answer {
        Value::Null => "nothing",
        Value::Bool(_) => "yes or no",
        Value::Number(number) if number.is_i64() => "a whole number",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) if PluginConfig::from_answer(answer).is_some() => "one plugin",
        Value::Object(_) => "an object",
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

    /// Whether this path is `prefix` or addresses something within what `prefix` addresses.
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(&prefix.0)
    }

    /// The path of the config or list holding what this path addresses, i.e. this path without
    /// its last segment. The parent of [`Self::ROOT`] is [`Self::ROOT`].
    pub(crate) fn parent(&self) -> Self {
        let mut parent = self.clone();
        parent.0.pop();
        parent
    }

    /// `error`, whose path is relative to what this path addresses, with its path seen from the
    /// root, see [`ConfigError::within`].
    pub(crate) fn prefix(&self, error: ConfigError) -> ConfigError {
        self.0
            .iter()
            .rev()
            .fold(error, |error, segment| error.within(segment.clone()))
    }

    /// Prefixes this path by `segment`, i.e. makes it the path of the same thing seen from one
    /// config further out.
    fn prepend(&mut self, segment: Segment) {
        self.0.insert(0, segment);
    }

    /// The path of what deserializing a config, or a plugin answer unless `is_config`, failed at,
    /// where `deserialized` is where in it as a [`Value`] that is.
    fn of_deserialized(deserialized: &serde_path_to_error::Path, is_config: bool) -> Self {
        use serde_path_to_error::Segment as Deserialized;

        // a config holds answers, a list holds answers, and a plugin answer holds its config under
        // "value"; its "type" is left out like anything not addressable as a field
        let mut path = Self::ROOT;
        let mut in_config = is_config;
        for segment in deserialized.iter() {
            match segment {
                Deserialized::Map { key } if in_config => {
                    path = path.then(Segment::field(key.as_str()));
                    in_config = false;
                }
                Deserialized::Map { key } if key == "value" => in_config = true,
                Deserialized::Seq { index } if !in_config => {
                    path = path.then(Segment::Index(*index));
                }
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
        let error = ConfigError::Rejected {
            path: Path::of("points_for_win"),
            reason: "cannot be negative".to_owned(),
        }
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
            .with("rounds", 3)
            .with("points_for_win", 2.0);

        assert_eq!(config.get("points_for_win"), Some(&Value::from(2.0)));
        assert_eq!(config.answers().count(), 2);
    }

    #[test]
    fn an_unset_field_is_unanswered() {
        let mut config = Config::new().with("third_place_match", true);
        config.unset("third_place_match");

        assert_eq!(config.get("third_place_match"), None);
    }

    #[test]
    fn a_field_answered_with_nothing_is_answered() {
        let config = Config::new().with("points_for_bye", Value::Null);

        assert_eq!(config.get("points_for_bye"), Some(&Value::Null));
    }

    #[test]
    fn a_plugin_answer_is_written_as_typetag_writes_an_adjacently_tagged_plugin() {
        let answer = PluginConfig::new("points", Config::new().with("for_win", 2.0));

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

    #[test]
    fn the_path_of_a_deserialization_error_leaves_out_how_plugins_are_written() {
        let config = serde_json::json!({
            "tie_breakers": [{"type": "points", "value": {"for_win": -1.0}}]
        });
        #[derive(Debug, Deserialize)]
        struct Points {
            #[serde(rename = "for_win")]
            _for_win: u32,
        }
        #[derive(Debug, Deserialize)]
        struct Tagged {
            #[serde(rename = "value")]
            _value: Points,
        }
        #[derive(Debug, Deserialize)]
        struct Ranker {
            #[serde(rename = "tie_breakers")]
            _tie_breakers: Vec<Tagged>,
        }

        let error = serde_path_to_error::deserialize::<_, Ranker>(&config).unwrap_err();

        assert_eq!(
            Path::of_deserialized(error.path(), true),
            Path::of("tie_breakers")
                .then(Segment::Index(0))
                .then(Segment::field("for_win"))
        );
    }

    #[test]
    fn the_path_of_a_deserialization_error_in_a_plugin_answer_is_relative_to_the_plugin() {
        let answer = serde_json::json!({"type": "points", "value": {"for_win": -1.0}});
        #[derive(Debug, Deserialize)]
        struct Points {
            #[serde(rename = "for_win")]
            _for_win: u32,
        }
        #[derive(Debug, Deserialize)]
        struct Tagged {
            #[serde(rename = "value")]
            _value: Points,
        }

        let error = serde_path_to_error::deserialize::<_, Tagged>(&answer).unwrap_err();

        assert_eq!(
            Path::of_deserialized(error.path(), false),
            Path::of("for_win")
        );
    }
}
