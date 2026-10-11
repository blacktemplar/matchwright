//! What a plugin has to be told in order to be built, so that a program can ask for it without
//! knowing the plugin.
//!
//! A [`Configurable`] states its questions as a [`Schema`] — a list of [`Field`]s, each taking an
//! answer of one [`FieldKind`] and offering a default — and builds itself from the answers, see
//! [`Config`]. A plugin registers in the [catalog](crate::catalog) with its schema, so that a
//! program reads the questions of a plugin before it is chosen and never names one.

use std::any::TypeId;

use crate::{
    catalog::PluginKind,
    config::{Config, ConfigError, Path, PluginConfig, Segment, Value},
};

/// Something configured by answering the fields of its [`Schema`]: a plugin, or the composite a
/// program configures plugins into.
pub trait Configurable: Sized + 'static {
    /// What has to be answered in order to build it.
    fn schema() -> Schema;

    /// Builds it from `config`, which answers every field of [`Self::schema`], see
    /// [`Schema::complete`].
    ///
    /// # Errors
    /// Returns [`ConfigError::rejected`] for an answer this cannot be built from, and whatever
    /// reading the answers fails with if `config` does not answer the whole schema.
    fn build(config: &Config) -> Result<Self, ConfigError>;

    /// The plugins this configuration suggests for the composite it is part of, which is what a
    /// field taking plugins of their kind offers as its default.
    ///
    /// A recommendation of a plugin of a kind the composite takes nowhere has no effect.
    fn recommends(&self) -> Vec<PluginConfig> {
        Vec::new()
    }
}

/// Everything a [`Configurable`] has to be told, in the order it is asked in.
#[derive(Debug, Default, Clone)]
pub struct Schema(Vec<Field>);

impl Schema {
    /// A schema asking for nothing, which is what a plugin without parameters has.
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// This schema with `field` asked after the fields it holds already.
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

    /// `config` with every field of this schema answered, i.e. the answers of `config` and the
    /// default of every field it leaves unanswered, which is what a [`Configurable`] is built
    /// from.
    ///
    /// The answer to a field taking plugins is kept as it is; the configuration of such a plugin
    /// is completed against the schema of that plugin when it is built.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if `config` answers a field this schema does not
    /// have, [`ConfigError::WrongKind`] if an answer is not one its field takes, anchored to where
    /// in a nested answer the mismatch is, and [`ConfigError::Unanswered`] if a field without a
    /// default is unanswered.
    pub fn complete(&self, config: &Config) -> Result<Config, ConfigError> {
        for (answered, _) in config.answers() {
            if self.field_named(answered).is_none() {
                return Err(ConfigError::UnknownField {
                    path: Path::of(answered),
                });
            }
        }
        let mut completed = Config::new();
        for field in self.fields() {
            let answer = match config.get(field.name()) {
                Some(answer) => answer,
                None => field
                    .default_value()
                    .ok_or_else(|| ConfigError::Unanswered {
                        path: Path::of(field.name()),
                    })?,
            };
            // the default is checked like an answer, so that a plugin offering one of the wrong
            // kind says so where it is configured rather than where it is built
            if let Some(error) = field.kind().reject(answer) {
                return Err(error.within(Segment::field(field.name())));
            }
            completed.set(field.name(), answer.clone());
        }
        Ok(completed)
    }
}

/// One question a [`Configurable`] asks, i.e. what is answered in a [`Config`] under its name.
///
/// A field with a default can be left unanswered; a [`Configurable`] all of whose fields have one
/// needs no configuration and can be created unasked, see
/// [`PluginEntry::build`](crate::catalog::PluginEntry::build).
#[derive(Debug, Clone)]
pub struct Field {
    name: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    kind: FieldKind,
    default: Option<Value>,
}

impl Field {
    /// A field answered by an answer of kind `kind`, named `name` and labelled `label`.
    pub const fn new(name: &'static str, label: &'static str, kind: FieldKind) -> Self {
        Self {
            name,
            label,
            description: None,
            kind,
            default: None,
        }
    }

    /// A field answered by yes or no, named `name` and labelled `label`, e.g. "Play a match for
    /// third place?".
    pub const fn bool(name: &'static str, label: &'static str) -> Self {
        Self::new(name, label, FieldKind::Bool)
    }

    /// A field answered by a whole number, named `name` and labelled `label`.
    pub const fn integer(name: &'static str, label: &'static str) -> Self {
        Self::new(name, label, FieldKind::Integer)
    }

    /// A field answered by a number with a fractional part, named `name` and labelled `label`.
    pub const fn decimal(name: &'static str, label: &'static str) -> Self {
        Self::new(name, label, FieldKind::Decimal)
    }

    /// A field answered by one plugin of kind `K`, named `name` and labelled `label`.
    pub const fn plugin<K: PluginKind>(name: &'static str, label: &'static str) -> Self {
        Self::new(name, label, FieldKind::plugin::<K>())
    }

    /// A field answered by any number of plugins of kind `K`, named `name` and labelled `label`.
    pub fn plugins<K: PluginKind>(name: &'static str, label: &'static str) -> Self {
        Self::new(name, label, FieldKind::plugins::<K>())
    }

    /// This field with `description` as the help text of its question.
    #[must_use]
    pub const fn described(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }

    /// This field with `default` as the answer it offers, which is what makes it one that can be
    /// left unanswered.
    #[must_use]
    pub fn with_default(mut self, default: impl Into<Value>) -> Self {
        self.default = Some(default.into());
        self
    }

    /// This field answered by what it takes or by nothing, see [`FieldKind::Optional`]. A default
    /// it has already becomes that answer, given.
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.kind = FieldKind::optional(self.kind);
        self.default = self
            .default
            .map(|default| Value::Optional(Some(Box::new(default))));
        self
    }

    /// This field answered by any number of answers of what it takes, see [`FieldKind::List`]. A
    /// default it has already becomes a list of that one answer.
    #[must_use]
    pub fn repeated(mut self) -> Self {
        self.kind = FieldKind::list(self.kind);
        self.default = self.default.map(|default| Value::List(vec![default]));
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

/// What kind of answer a [`Field`] takes.
///
/// The scalar kinds are read as the Rust types of [`Answer`](crate::config::Answer), a list as a
/// `Vec` of them and an optional answer as an `Option`, see [`Config::read`]. A field taking
/// plugins holds the kind of
/// the plugins it takes as the [`TypeId`] of its [`PluginKind`] and is read with
/// [`Config::plugin`], [`Config::plugins`] or [`Config::optional_plugin`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// Yes or no, answered by [`Value::Bool`].
    Bool,
    /// A whole number, answered by [`Value::Integer`].
    Integer,
    /// A number with a fractional part, answered by [`Value::Decimal`] or by a whole number.
    Decimal,
    /// One plugin of the kind, answered by [`Value::Plugin`].
    Plugin(TypeId),
    /// Any number of answers of the kind, answered by [`Value::List`].
    List(Box<FieldKind>),
    /// An answer of the kind or nothing, answered by [`Value::Optional`].
    ///
    /// Being answered with nothing is being answered; a field that may be left unanswered needs a
    /// default, which for such a field is usually [`Value::NOTHING`].
    Optional(Box<FieldKind>),
}

impl FieldKind {
    /// One plugin of kind `K`.
    pub const fn plugin<K: PluginKind>() -> Self {
        Self::Plugin(TypeId::of::<K>())
    }

    /// Any number of plugins of kind `K`.
    pub fn plugins<K: PluginKind>() -> Self {
        Self::list(Self::plugin::<K>())
    }

    /// Any number of answers of the kind `element`.
    pub fn list(element: Self) -> Self {
        Self::List(Box::new(element))
    }

    /// An answer of the kind `answer` or nothing.
    pub fn optional(answer: Self) -> Self {
        Self::Optional(Box::new(answer))
    }

    /// Whether `answer` is one a field of this kind takes.
    pub fn takes(&self, answer: &Value) -> bool {
        self.reject(answer).is_none()
    }

    /// Why `answer` is not one a field of this kind takes, or [`None`] if it is, as the config
    /// holding that field sees it, i.e. to be prefixed by the field.
    pub(crate) fn reject(&self, answer: &Value) -> Option<ConfigError> {
        match (self, answer) {
            (Self::Bool, Value::Bool(_))
            | (Self::Integer, Value::Integer(_))
            | (Self::Decimal, Value::Decimal(_) | Value::Integer(_))
            | (Self::Plugin(_), Value::Plugin(_))
            | (Self::Optional(_), Value::Optional(None)) => None,
            (Self::Optional(inner), Value::Optional(Some(answer))) => inner.reject(answer),
            (Self::List(element), Value::List(answers)) => {
                answers.iter().enumerate().find_map(|(position, answer)| {
                    element
                        .reject(answer)
                        .map(|error| error.within(Segment::Index(position)))
                })
            }
            _ => Some(ConfigError::WrongKind {
                path: Path::ROOT,
                expected: self.name(),
                found: answer.name(),
            }),
        }
    }

    /// What this kind of answer is called in an error message.
    pub(crate) fn name(&self) -> &'static str {
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
    use std::fmt::Debug;

    use super::*;
    use crate::{Plugin, catalog::PluginEntry, inventory, plugin_kind};

    /// A plugin axis whose plugins are nothing but their configuration, reported as a summary so
    /// that a built plugin can be checked without being downcast.
    trait Configured: Plugin + Debug {
        fn summary(&self) -> String;
    }

    plugin_kind! {
        /// The axis of [`Configured`].
        ConfiguredKind for dyn Configured
    }

    /// A plugin with a field of every scalar kind and one that may be answered with nothing, all
    /// with a default.
    #[derive(Debug)]
    struct Points {
        for_win: f64,
        rounds: i64,
        counts_byes: bool,
        for_bye: Option<f64>,
    }

    impl Plugin for Points {
        fn label() -> &'static str {
            "points"
        }

        fn description() -> &'static str {
            "a plugin with a parameter of every scalar kind"
        }
    }

    impl Configurable for Points {
        fn schema() -> Schema {
            Schema::new()
                .field(
                    Field::decimal("for_win", "Points for a win")
                        .described("what winning a match is worth")
                        .with_default(1.0),
                )
                .field(Field::integer("rounds", "Rounds").with_default(3i64))
                .field(Field::bool("counts_byes", "Count byes").with_default(true))
                .field(
                    Field::decimal("for_bye", "Points for a bye")
                        .with_default(0.5)
                        .optional(),
                )
        }

        fn build(config: &Config) -> Result<Self, ConfigError> {
            let for_win = config.read("for_win")?;
            if for_win < 0.0 {
                return Err(ConfigError::rejected(
                    "for_win",
                    "the points for a win cannot be negative",
                ));
            }
            Ok(Self {
                for_win,
                rounds: config.read("rounds")?,
                counts_byes: config.read("counts_byes")?,
                for_bye: config.read("for_bye")?,
            })
        }

        fn recommends(&self) -> Vec<PluginConfig> {
            vec![PluginConfig::of::<Matches>(Config::new())]
        }
    }

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

    inventory::submit! { ConfiguredKind::register::<Points>() }

    /// A plugin asking for nothing.
    #[derive(Debug)]
    struct Matches;

    impl Plugin for Matches {
        fn label() -> &'static str {
            "matches"
        }

        fn description() -> &'static str {
            "a plugin without parameters"
        }
    }

    impl Configurable for Matches {
        fn schema() -> Schema {
            Schema::new()
        }

        fn build(_: &Config) -> Result<Self, ConfigError> {
            Ok(Self)
        }
    }

    impl Configured for Matches {
        fn summary(&self) -> String {
            "matches".to_owned()
        }
    }

    inventory::submit! { ConfiguredKind::register::<Matches>() }

    /// A plugin with a field without a default, so that it has to be configured, and one taking a
    /// list of numbers.
    #[derive(Debug)]
    struct Threshold {
        least: i64,
        weights: Vec<f64>,
    }

    impl Plugin for Threshold {
        fn label() -> &'static str {
            "threshold"
        }

        fn description() -> &'static str {
            "a plugin that has to be configured"
        }
    }

    impl Configurable for Threshold {
        fn schema() -> Schema {
            Schema::new()
                .field(Field::integer("least", "Matches at least"))
                .field(
                    Field::decimal("weights", "Weight per round")
                        .repeated()
                        .with_default(Value::EMPTY_LIST),
                )
        }

        fn build(config: &Config) -> Result<Self, ConfigError> {
            Ok(Self {
                least: config.read("least")?,
                weights: config.read("weights")?,
            })
        }
    }

    impl Configured for Threshold {
        fn summary(&self) -> String {
            let Self { least, weights } = self;
            format!("threshold({least}, {weights:?})")
        }
    }

    inventory::submit! { ConfiguredKind::register::<Threshold>() }

    /// A plugin made of other plugins, i.e. one with a field taking one plugin, one taking a list
    /// of them and one taking a plugin or nothing.
    #[derive(Debug)]
    struct Ranker {
        by: Box<dyn Configured>,
        tie_breakers: Vec<Box<dyn Configured>>,
        fallback: Option<Box<dyn Configured>>,
    }

    impl Plugin for Ranker {
        fn label() -> &'static str {
            "ranker"
        }

        fn description() -> &'static str {
            "a plugin made of other plugins"
        }
    }

    impl Configurable for Ranker {
        fn schema() -> Schema {
            Schema::new()
                .field(
                    Field::plugin::<ConfiguredKind>("by", "Rank by")
                        .with_default(PluginConfig::of::<Matches>(Config::new())),
                )
                .field(
                    Field::plugins::<ConfiguredKind>("tie_breakers", "Tie breakers")
                        .with_default(Value::EMPTY_LIST),
                )
                .field(
                    Field::plugin::<ConfiguredKind>("fallback", "Fall back to")
                        .optional()
                        .with_default(Value::NOTHING),
                )
        }

        fn build(config: &Config) -> Result<Self, ConfigError> {
            Ok(Self {
                by: config.plugin::<ConfiguredKind>("by")?,
                tie_breakers: config.plugins::<ConfiguredKind>("tie_breakers")?,
                fallback: config.optional_plugin::<ConfiguredKind>("fallback")?,
            })
        }
    }

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

    inventory::submit! { ConfiguredKind::register::<Ranker>() }

    /// A plugin of no axis at all, so that a config can name one no kind has registered.
    struct Unregistered;

    impl Configurable for Unregistered {
        fn schema() -> Schema {
            Schema::new()
        }

        fn build(_: &Config) -> Result<Self, ConfigError> {
            Ok(Self)
        }
    }

    /// The entry of the plugin `P`.
    fn entry_of<P: Configurable>() -> &'static PluginEntry<ConfiguredKind> {
        ConfiguredKind::entries()
            .find(|entry| entry.id() == TypeId::of::<P>())
            .expect("the plugin is registered")
    }

    /// The summary of the plugin `P` as `config` configures it.
    fn built<P: Configurable>(config: Config) -> Result<String, ConfigError> {
        entry_of::<P>()
            .build(&config)
            .map(|plugin| plugin.summary())
    }

    #[test]
    fn a_field_taking_plugins_knows_the_kind_it_takes() {
        let schema = Ranker::schema();
        let kind = FieldKind::plugin::<ConfiguredKind>();

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
    }

    #[test]
    fn a_default_survives_the_field_it_is_given_to_becoming_optional_or_repeated() {
        let optional = Field::decimal("for_bye", "Points for a bye")
            .with_default(0.5)
            .optional();
        let repeated = Field::decimal("weights", "Weight per round")
            .with_default(0.5)
            .repeated();

        assert_eq!(optional.default_value(), Some(&Value::from(Some(0.5))));
        assert_eq!(repeated.default_value(), Some(&Value::from(vec![0.5])));
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
            .with("for_bye", Value::NOTHING);

        assert_eq!(
            built::<Points>(config).as_deref(),
            Ok("points(2.5, 3, false, none)")
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
            built::<Threshold>(Config::new().with("least", 2i64)).as_deref(),
            Ok("threshold(2, [])")
        );
    }

    #[test]
    fn a_field_taking_a_list_is_answered_by_any_number_of_answers() {
        let config = Config::new()
            .with("least", 2i64)
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
            built::<Points>(Config::new().with("counts_byes", 1i64)),
            Err(ConfigError::WrongKind {
                path: Path::of("counts_byes"),
                expected: "yes or no",
                found: "a whole number",
            })
        );
    }

    #[test]
    fn an_answer_of_another_kind_in_a_list_is_reported_by_its_position() {
        let config = Config::new()
            .with("least", 2i64)
            .with("weights", vec![Value::Decimal(1.0), Value::Bool(true)]);

        assert_eq!(
            built::<Threshold>(config),
            Err(ConfigError::WrongKind {
                path: Path::of("weights").then(Segment::Index(1)),
                expected: "a number",
                found: "yes or no",
            })
        );
    }

    #[test]
    fn a_default_of_another_kind_than_its_field_is_an_error() {
        let schema = Schema::new().field(Field::integer("rounds", "Rounds").with_default(true));

        assert_eq!(
            schema.complete(&Config::new()),
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
            Err(ConfigError::rejected(
                "for_win",
                "the points for a win cannot be negative"
            ))
        );
    }

    #[test]
    fn a_field_taking_plugins_is_built_through_the_catalog() {
        let config = Config::new()
            .with(
                "by",
                PluginConfig::of::<Points>(Config::new().with("for_win", 2.0)),
            )
            .with(
                "tie_breakers",
                vec![
                    PluginConfig::of::<Matches>(Config::new()),
                    PluginConfig::of::<Threshold>(Config::new().with("least", 4i64)),
                ],
            )
            .with("fallback", Some(PluginConfig::of::<Matches>(Config::new())));

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
        let config = Config::new().with("fallback", Value::NOTHING);

        assert_eq!(
            built::<Ranker>(config).as_deref(),
            Ok("ranker(matches, [], none)")
        );
    }

    #[test]
    fn a_plugin_answering_a_field_that_takes_none_is_an_error() {
        let config = Config::new().with("by", 1i64);

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
        let config = Config::new().with(
            "by",
            PluginConfig::of::<Points>(Config::new().with("for_win", -1.0)),
        );

        assert_eq!(
            built::<Ranker>(config),
            Err(ConfigError::Rejected {
                path: Path::of("by").then(Segment::field("for_win")),
                reason: "the points for a win cannot be negative".to_owned(),
            })
        );
    }

    #[test]
    fn an_error_in_one_of_several_nested_plugins_is_reported_by_its_position() {
        let config = Config::new().with(
            "tie_breakers",
            vec![
                PluginConfig::of::<Matches>(Config::new()),
                PluginConfig::of::<Threshold>(Config::new()),
            ],
        );

        assert_eq!(
            built::<Ranker>(config),
            Err(ConfigError::Unanswered {
                path: Path::of("tie_breakers")
                    .then(Segment::Index(1))
                    .then(Segment::field("least")),
            })
        );
    }

    #[test]
    fn an_error_in_a_nested_plugin_answering_an_optional_field_keeps_its_field() {
        let config = Config::new().with(
            "fallback",
            Some(PluginConfig::of::<Points>(
                Config::new().with("for_win", -1.0),
            )),
        );

        assert_eq!(
            built::<Ranker>(config),
            Err(ConfigError::Rejected {
                path: Path::of("fallback").then(Segment::field("for_win")),
                reason: "the points for a win cannot be negative".to_owned(),
            })
        );
    }

    #[test]
    fn a_plugin_no_kind_registered_cannot_answer_a_plugin_field() {
        let config = Config::new().with("by", PluginConfig::of::<Unregistered>(Config::new()));

        assert_eq!(
            built::<Ranker>(config),
            Err(ConfigError::UnknownPlugin {
                path: Path::of("by"),
            })
        );
    }

    #[test]
    fn a_plugin_recommends_what_a_field_of_that_kind_offers() {
        let points = Points::build(&Points::schema().complete(&Config::new()).unwrap()).unwrap();

        assert_eq!(
            points.recommends(),
            vec![PluginConfig::of::<Matches>(Config::new())]
        );
        assert!(Matches.recommends().is_empty());
    }
}
