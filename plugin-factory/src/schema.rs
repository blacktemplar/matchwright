//! What a plugin has to be told in order to be built, so that a program can ask for it without
//! knowing the plugin.
//!
//! A [`Configurable`] states its questions as a [`Schema`] — a list of [`Field`]s, each taking an
//! answer of one [`FieldKind`] and offering a default — and is deserialized from the answers, see
//! [`Config`]. A plugin registers in the [catalog](crate::catalog) with its schema, so that a
//! program reads the questions of a plugin before it is chosen and never names one.

pub use plugin_factory_derive::Configurable;
use serde::de::DeserializeOwned;

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
/// It is built by deserializing its answers, with every field left unanswered answered by its
/// default. A struct without fields may be a unit struct, which is deserialized from `null`
/// rather than from an empty config. An answer it rejects, e.g. through
/// `#[serde(deserialize_with = ..)]` or a field type's `#[serde(try_from = ..)]`, is reported as
/// [`ConfigError::Rejected`] with the path of the answer.
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
/// takes and which plugins it holds.
///
/// Implemented for `bool`, the integer and floating point types, `Box<dyn Trait>` for the trait of
/// a plugin axis, and `Option` and `Vec` of any of them. A type that deserializes from one of
/// these answers, such as a newtype checking its value, implements it to be a field.
pub trait FieldType {
    /// The kind of answer a field of this type takes.
    fn kind() -> FieldKind;

    /// What `serde` deserializes a missing field of this type as, which is the default of the
    /// field, or [`None`] if a missing field is an error; only an `Option` has one.
    fn missing() -> Option<Value> {
        None
    }

    /// Adds the plugins `self` holds to `plugins`; none by default.
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

    fn missing() -> Option<Value> {
        Some(Value::Null)
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

/// Everything a [`Configurable`] has to be told, in the order it is asked in.
#[derive(Debug, Clone)]
pub struct Schema(Vec<Field>);

impl Schema {
    /// A schema asking for nothing. Called by the derive; not part of the API.
    #[doc(hidden)]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// This schema with `field` asked after the fields it holds already. Called by the derive;
    /// not part of the API.
    #[doc(hidden)]
    #[must_use]
    pub fn field(mut self, field: Field) -> Self {
        self.0.push(field);
        self
    }

    /// The fields of this schema, in the order they are asked in.
    pub fn fields(&self) -> &[Field] {
        &self.0
    }

    /// The field named `name`, or [`None`] if the schema has none.
    pub fn field_named(&self, name: &str) -> Option<&Field> {
        self.0.iter().find(|field| field.name() == name)
    }

    /// `config`, the config at `at`, checked against this schema, and so the config of every
    /// plugin answering one of its fields against the schema of that plugin, down the whole tree.
    ///
    /// If `fill_defaults`, every field left unanswered is answered by its default, which is what
    /// a [`Configurable`] is built from; otherwise it is left unanswered. A plugin answer is
    /// written as [`AnyAxis::walk`] writes it.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if a config answers a field its schema does not
    /// have, [`ConfigError::WrongKind`] if an answer is not one its field takes,
    /// [`ConfigError::UnknownPlugin`] if a plugin answering a field is not registered for its
    /// axis, and, if `fill_defaults`, [`ConfigError::Unanswered`] if a field without a default is
    /// unanswered, each with its path from the root.
    pub(crate) fn walk(
        &self,
        config: &Config,
        at: &Path,
        fill_defaults: bool,
    ) -> Result<Config, ConfigError> {
        if let Some((answered, _)) = config
            .answers()
            .find(|(answered, _)| self.field_named(answered).is_none())
        {
            return Err(ConfigError::UnknownField {
                path: at.clone().field(answered),
            });
        }
        let mut walked = Config::new();
        for field in self.fields() {
            let path = at.clone().field(field.name());
            let answer = match config.get(field.name()) {
                Some(answer) => answer,
                None if !fill_defaults => continue,
                None => field
                    .default_value()
                    .ok_or_else(|| ConfigError::Unanswered { path: path.clone() })?,
            };
            // the default is checked like an answer, so that a plugin offering one of the wrong
            // kind says so where it is configured rather than where it is built
            walked.set(
                field.name(),
                field.kind().walk(answer, &path, fill_defaults)?,
            );
        }
        Ok(walked)
    }
}

/// One question a [`Configurable`] asks, i.e. what is answered in a [`Config`] under its name.
///
/// A field with a default can be left unanswered; a [`Configurable`] all of whose fields have one
/// needs no configuration and can be created unasked, see
/// [`PluginConfig::build`].
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

    /// This field with `default` as the answer it offers, which is what makes it one that can be
    /// left unanswered. Called by the derive; not part of the API.
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

    /// The answer this field offers, or [`None`] if it has to be answered.
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
    /// Any number of answers of the kind, answered by a [`Value::Array`].
    List(Box<FieldKind>),
    /// An answer of the kind or nothing, answered by an answer of the kind or by [`Value::Null`].
    ///
    /// Being answered with nothing is being answered; a field that may be left unanswered needs a
    /// default, which for such a field is usually [`Value::Null`]. An optional answer of an
    /// optional answer is just an optional answer.
    Optional(Box<FieldKind>),
}

impl FieldKind {
    /// One plugin of the axis `A`.
    pub const fn plugin<A: Axis + ?Sized>() -> Self {
        Self::Plugin(AnyAxis::of::<A>())
    }

    /// Any number of answers of the kind `element`.
    pub fn list(element: Self) -> Self {
        Self::List(Box::new(element))
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
            Self::List(inner) | Self::Optional(inner) => inner.axis(),
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
        fill_defaults: bool,
    ) -> Result<Value, ConfigError> {
        match (self, answer) {
            (Self::Bool, Value::Bool(_)) | (Self::Optional(_), Value::Null) => Ok(answer.clone()),
            (Self::Integer, Value::Number(number)) if number.is_i64() => Ok(answer.clone()),
            (Self::Decimal, Value::Number(_)) => Ok(answer.clone()),
            (Self::Optional(inner), _) => inner.walk(answer, at, fill_defaults),
            (Self::List(element), Value::Array(answers)) => answers
                .iter()
                .enumerate()
                .map(|(position, answer)| {
                    element.walk(answer, &at.clone().index(position), fill_defaults)
                })
                .collect::<Result<_, _>>()
                .map(Value::Array),
            (Self::Plugin(axis), _) => match PluginConfig::from_answer(answer) {
                Some(plugin) => axis.walk(&plugin, at, fill_defaults),
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
            Self::List(_) => "a list",
            Self::Optional(_) => "an optional answer",
        }
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
        #[plugin(label = "Fall back to")]
        fallback: Option<Box<dyn Configured>>,
    }

    fn matches() -> Box<dyn Configured> {
        Box::new(Matches)
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
        <dyn Configured>::entry(P::ID).expect("the plugin is registered")
    }

    /// The summary of the plugin `P` as `config` configures it.
    fn built<P: PluginType>(config: Config) -> Result<String, ConfigError> {
        PluginConfig::configured(P::ID, config)
            .build::<dyn Configured>()
            .map(|plugin| plugin.summary())
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
    fn an_unanswered_field_is_answered_by_its_default() {
        assert_eq!(
            built::<Points>(Config::new()).as_deref(),
            Ok("points(1, 3, true, 0.5)")
        );
    }

    #[test]
    fn an_answer_is_kept_as_it_is_given() {
        let config = Config::new()
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
            built::<Points>(Config::new().with("for_win", 2)).as_deref(),
            Ok("points(2, 3, true, 0.5)")
        );
    }

    #[test]
    fn a_field_without_a_default_has_to_be_answered() {
        assert_eq!(
            built::<Threshold>(Config::new()),
            Err(ConfigError::Unanswered {
                path: Path::of("least"),
            })
        );
        assert_eq!(
            built::<Threshold>(Config::new().with("least", 2)).as_deref(),
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
            built::<Points>(Config::new().with("counts_byes", 1)),
            Err(ConfigError::WrongKind {
                path: Path::of("counts_byes"),
                expected: "yes or no",
                found: "a whole number",
            })
        );
        assert_eq!(
            built::<Points>(Config::new().with("rounds", 2.5)),
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
    fn a_default_of_another_kind_than_its_field_is_an_error() {
        // a derived schema cannot have such a default, so it is made by hand
        let schema = Schema::new()
            .field(Field::new("rounds", "Rounds", FieldKind::Integer).with_default(true));

        assert_eq!(
            schema.walk(&Config::new(), &Path::ROOT, true),
            Err(ConfigError::WrongKind {
                path: Path::of("rounds"),
                expected: "a whole number",
                found: "yes or no",
            })
        );
    }

    #[test]
    fn an_answer_a_plugin_rejects_is_reported_as_its_field() {
        assert_eq!(
            built::<Points>(Config::new().with("for_win", -1.0)),
            Err(ConfigError::Rejected {
                path: Path::of("for_win"),
                reason: "the points for a win cannot be negative".to_owned(),
            })
        );
    }

    #[test]
    fn a_field_taking_plugins_is_built_through_the_catalog() {
        let config = Config::new()
            .with("by", PluginConfig::of::<Points>().with("for_win", 2.0))
            .with(
                "tie_breakers",
                vec![
                    PluginConfig::of::<Matches>(),
                    PluginConfig::of::<Threshold>().with("least", 4),
                ],
            )
            .with("fallback", PluginConfig::of::<Matches>());

        assert_eq!(
            built::<Ranker>(config).as_deref(),
            Ok("ranker(points(2, 3, true, 0.5), [matches, threshold(4, [])], matches)")
        );
    }

    #[test]
    fn a_plugin_field_left_unanswered_is_answered_by_its_default() {
        assert_eq!(
            built::<Ranker>(Config::new()).as_deref(),
            Ok("ranker(matches, [], none)")
        );
    }

    #[test]
    fn a_plugin_field_that_may_be_answered_with_nothing_takes_no_plugin() {
        let config = Config::new().with("fallback", Value::Null);

        assert_eq!(
            built::<Ranker>(config).as_deref(),
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
        let config = Config::new().with("by", PluginConfig::of::<Points>().with("for_win", -1.0));

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
        let config = Config::new().with(
            "tie_breakers",
            vec![
                PluginConfig::of::<Matches>(),
                PluginConfig::of::<Threshold>(),
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
        let config = Config::new().with(
            "fallback",
            PluginConfig::of::<Points>().with("for_win", -1.0),
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
    fn a_built_plugin_serializes_as_the_completed_config_it_is_built_from() {
        let config = Config::new()
            .with("by", PluginConfig::of::<Points>().with("for_win", 2.0))
            .with("fallback", PluginConfig::of::<Matches>());
        let completed = Ranker::schema().walk(&config, &Path::ROOT, true).unwrap();
        let built = PluginConfig::configured(Ranker::ID, config)
            .build::<dyn Configured>()
            .unwrap();

        assert_eq!(
            completed.get("fallback"),
            Some(&json!({"type": "matches", "value": null}))
        );
        assert_eq!(
            serde_json::to_value(&built).ok(),
            Some(PluginConfig::configured(Ranker::ID, completed).into())
        );
    }

    #[test]
    fn a_plugin_without_parameters_is_completed_as_a_unit_struct_is_written() {
        let walked = |entry: &PluginEntry, config, fill_defaults| {
            entry.walk(&config, &Path::ROOT, fill_defaults)
        };

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
                Config::new().with("least", 2),
                true
            ),
            Ok(json!({"least": 2, "weights": []}))
        );
    }

    #[test]
    fn an_answer_left_unanswered_is_no_error_unless_the_defaults_are_filled() {
        let config = Config::new().with(
            "tie_breakers",
            vec![PluginConfig::of::<Threshold>().with("weights", vec![1.0])],
        );

        assert_eq!(
            Ranker::schema()
                .walk(&config, &Path::ROOT, false)
                .map(Value::from),
            Ok(json!({
                "tie_breakers": [{"type": "threshold", "value": {"weights": [1.0]}}]
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
