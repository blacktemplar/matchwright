//! What a plugin has to be told in order to be built, so that a program can ask for it without
//! knowing the plugin.
//!
//! A [`Configurable`] states its questions as a [`Schema`] — a list of [`Field`]s, each taking an
//! answer of one [`FieldKind`] and offering a default — and is deserialized from the answers, see
//! [`Config`]. A plugin states its schema in [`Plugin::schema`] and registers in the
//! [catalog](crate::catalog) with it, so that a program reads the questions of a plugin before it
//! is chosen and never names one.

use serde::de::DeserializeOwned;

use crate::{
    Plugin,
    catalog::{Kind, PluginKind, PluginObject},
    config::{self, Config, ConfigError, Path, PluginConfig, Segment, Value},
};

/// Something configured by answering the fields of its [`Schema`]: a plugin, or the composite a
/// program configures plugins into.
///
/// Implemented for every [`Plugin`] with a `Deserialize` impl, through [`Plugin::schema`]; a
/// composite implements it itself.
///
/// It is built by deserializing a config that answers every field of its schema, see
/// [`Schema::complete`], so its `Deserialize` impl has to read each field under the name the
/// schema gives it. A plugin without parameters may be a unit struct, which is deserialized from
/// `null` rather than from an empty config. An answer it rejects, e.g. through
/// `#[serde(deserialize_with = ..)]` or `#[serde(try_from = ..)]`, is reported as
/// [`ConfigError::Rejected`] with the path of the answer.
pub trait Configurable: Sized + DeserializeOwned + 'static {
    /// What has to be answered in order to build it.
    fn schema() -> Schema;
}

impl<P: Plugin + DeserializeOwned> Configurable for P {
    fn schema() -> Schema {
        <P as Plugin>::schema()
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

impl<O: PluginObject + ?Sized> FieldType for Box<O> {
    fn kind() -> FieldKind {
        FieldKind::plugin::<O::Kind>()
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
    /// The config of a plugin answering a field is completed against the schema of that plugin
    /// in turn, so that the whole tree is complete.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if `config` answers a field this schema does not
    /// have, [`ConfigError::WrongKind`] if an answer is not one its field takes,
    /// [`ConfigError::UnknownPlugin`] if a plugin answering a field is not registered for its
    /// kind, and [`ConfigError::Unanswered`] if a field without a default is unanswered, each
    /// anchored to where in a nested answer it is.
    pub fn complete(&self, config: &Config) -> Result<Config, ConfigError> {
        self.check_known(config)?;
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
            let answer = field
                .kind()
                .complete(answer)
                .map_err(|error| error.within(Segment::field(field.name())))?;
            completed.set(field.name(), answer);
        }
        Ok(completed)
    }

    /// `config` with every answer checked as [`Self::complete`] checks it, but with the fields it
    /// leaves unanswered left unanswered, here and in the configs of the plugins answering its
    /// fields. A plugin answer is written as [`PluginConfig`] writes it.
    ///
    /// # Errors
    /// Returns what [`Self::complete`] returns, except [`ConfigError::Unanswered`].
    pub(crate) fn check(&self, config: &Config) -> Result<Config, ConfigError> {
        self.check_known(config)?;
        let mut checked = Config::new();
        for field in self.fields() {
            if let Some(answer) = config.get(field.name()) {
                let answer = field
                    .kind()
                    .check(answer)
                    .map_err(|error| error.within(Segment::field(field.name())))?;
                checked.set(field.name(), answer);
            }
        }
        Ok(checked)
    }

    /// Checks that `config` answers no field this schema does not have.
    fn check_known(&self, config: &Config) -> Result<(), ConfigError> {
        match config
            .answers()
            .find(|(answered, _)| self.field_named(answered).is_none())
        {
            Some((answered, _)) => Err(ConfigError::UnknownField {
                path: Path::of(answered),
            }),
            None => Ok(()),
        }
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
    /// it has already stays its default.
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.kind = FieldKind::optional(self.kind);
        self
    }

    /// This field answered by any number of answers of what it takes, see [`FieldKind::List`]. A
    /// default it has already becomes a list of that one answer.
    #[must_use]
    pub fn repeated(mut self) -> Self {
        self.kind = FieldKind::list(self.kind);
        self.default = self.default.map(|default| Value::Array(vec![default]));
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
    /// One plugin of the kind, answered by a [`PluginConfig`] naming a plugin registered for it.
    Plugin(Kind),
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
    /// One plugin of kind `K`.
    pub const fn plugin<K: PluginKind>() -> Self {
        Self::Plugin(Kind::of::<K>())
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

    /// The kind of plugin this kind of answer is made of, through lists and optional answers, or
    /// [`None`] if it takes no plugin.
    pub fn plugin_kind(&self) -> Option<Kind> {
        match self {
            Self::Plugin(kind) => Some(*kind),
            Self::List(inner) | Self::Optional(inner) => inner.plugin_kind(),
            Self::Bool | Self::Integer | Self::Decimal => None,
        }
    }

    /// `answer` with the config of every plugin in it completed against the schema of that
    /// plugin, see [`Schema::complete`].
    ///
    /// # Errors
    /// Returns why `answer` is not one a field of this kind takes, as the config holding that
    /// field sees it, i.e. to be prefixed by the field.
    fn complete(&self, answer: &Value) -> Result<Value, ConfigError> {
        self.map_plugins(answer, &|kind, plugin| kind.complete(plugin))
    }

    /// `answer` checked against this kind, with the config of every plugin in it checked against
    /// the schema of that plugin but not completed, see [`Schema::check`].
    ///
    /// # Errors
    /// Returns why `answer` is not one a field of this kind takes, as the config holding that
    /// field sees it, i.e. to be prefixed by the field.
    pub(crate) fn check(&self, answer: &Value) -> Result<Value, ConfigError> {
        self.map_plugins(answer, &|kind, plugin| kind.check(plugin))
    }

    /// `answer` checked against this kind, with every plugin answer in it replaced by what
    /// `plugin` makes of it.
    fn map_plugins(
        &self,
        answer: &Value,
        plugin: &dyn Fn(&Kind, &PluginConfig) -> Result<Value, ConfigError>,
    ) -> Result<Value, ConfigError> {
        match (self, answer) {
            (Self::Bool, Value::Bool(_)) | (Self::Optional(_), Value::Null) => Ok(answer.clone()),
            (Self::Integer, Value::Number(number)) if number.is_i64() => Ok(answer.clone()),
            (Self::Decimal, Value::Number(_)) => Ok(answer.clone()),
            (Self::Optional(inner), _) => inner.map_plugins(answer, plugin),
            (Self::List(element), Value::Array(answers)) => answers
                .iter()
                .enumerate()
                .map(|(position, answer)| {
                    element
                        .map_plugins(answer, plugin)
                        .map_err(|error| error.within(Segment::Index(position)))
                })
                .collect::<Result<_, _>>()
                .map(Value::Array),
            (Self::Plugin(kind), _) => match PluginConfig::from_answer(answer) {
                Some(answer) => plugin(kind, &answer),
                None => Err(self.wrong_kind(answer)),
            },
            _ => Err(self.wrong_kind(answer)),
        }
    }

    /// The error of `answer` not being of this kind, as the config holding the field sees it.
    fn wrong_kind(&self, answer: &Value) -> ConfigError {
        ConfigError::WrongKind {
            path: Path::ROOT,
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
    use std::fmt::Debug;

    use serde::{Deserialize, Deserializer, Serialize, de::Error};
    use serde_json::json;

    use super::*;
    use crate::{
        Plugin,
        catalog::{PluginEntry, check_serialization},
        inventory, plugin_kind,
    };

    /// A plugin axis whose plugins are nothing but their configuration, reported as a summary so
    /// that a built plugin can be checked without being downcast.
    #[typetag::serde(tag = "type", content = "value")]
    trait Configured: Plugin + Debug {
        fn summary(&self) -> String;
    }

    plugin_kind! {
        /// The axis of [`Configured`].
        ConfiguredKind for dyn Configured
    }

    /// A plugin with a field of every scalar kind and one that may be answered with nothing, all
    /// with a default.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Points {
        #[serde(deserialize_with = "non_negative")]
        for_win: f64,
        rounds: i64,
        counts_byes: bool,
        for_bye: Option<f64>,
    }

    fn non_negative<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let points = f64::deserialize(deserializer)?;
        if points < 0.0 {
            return Err(D::Error::custom("the points for a win cannot be negative"));
        }
        Ok(points)
    }

    impl Plugin for Points {
        fn id() -> &'static str {
            "points"
        }

        fn label() -> &'static str {
            "points"
        }

        fn description() -> &'static str {
            "a plugin with a parameter of every scalar kind"
        }

        fn schema() -> Schema {
            Schema::new()
                .field(
                    Field::decimal("for_win", "Points for a win")
                        .described("what winning a match is worth")
                        .with_default(1.0),
                )
                .field(Field::integer("rounds", "Rounds").with_default(3))
                .field(Field::bool("counts_byes", "Count byes").with_default(true))
                .field(
                    Field::decimal("for_bye", "Points for a bye")
                        .with_default(0.5)
                        .optional(),
                )
        }

        fn recommends(&self) -> Vec<PluginConfig> {
            vec![PluginConfig::of::<Matches>(Config::new())]
        }
    }

    #[typetag::serde(name = "points")]
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
    #[derive(Debug, Serialize, Deserialize)]
    struct Matches;

    impl Plugin for Matches {
        fn id() -> &'static str {
            "matches"
        }

        fn label() -> &'static str {
            "matches"
        }

        fn description() -> &'static str {
            "a plugin without parameters"
        }
    }

    #[typetag::serde(name = "matches")]
    impl Configured for Matches {
        fn summary(&self) -> String {
            "matches".to_owned()
        }
    }

    inventory::submit! { ConfiguredKind::register::<Matches>() }

    /// A plugin with a field without a default, so that it has to be configured, and one taking a
    /// list of numbers.
    #[derive(Debug, Serialize, Deserialize)]
    struct Threshold {
        least: i64,
        weights: Vec<f64>,
    }

    impl Plugin for Threshold {
        fn id() -> &'static str {
            "threshold"
        }

        fn label() -> &'static str {
            "threshold"
        }

        fn description() -> &'static str {
            "a plugin that has to be configured"
        }

        fn schema() -> Schema {
            Schema::new()
                .field(Field::integer("least", "Matches at least"))
                .field(
                    Field::decimal("weights", "Weight per round")
                        .repeated()
                        .with_default(json!([])),
                )
        }
    }

    #[typetag::serde(name = "threshold")]
    impl Configured for Threshold {
        fn summary(&self) -> String {
            let Self { least, weights } = self;
            format!("threshold({least}, {weights:?})")
        }
    }

    inventory::submit! { ConfiguredKind::register::<Threshold>() }

    /// A plugin made of other plugins, i.e. one with a field taking one plugin, one taking a list
    /// of them and one taking a plugin or nothing.
    #[derive(Debug, Serialize, Deserialize)]
    struct Ranker {
        by: Box<dyn Configured>,
        tie_breakers: Vec<Box<dyn Configured>>,
        fallback: Option<Box<dyn Configured>>,
    }

    impl Plugin for Ranker {
        fn id() -> &'static str {
            "ranker"
        }

        fn label() -> &'static str {
            "ranker"
        }

        fn description() -> &'static str {
            "a plugin made of other plugins"
        }

        fn schema() -> Schema {
            Schema::new()
                .field(
                    Field::plugin::<ConfiguredKind>("by", "Rank by")
                        .with_default(PluginConfig::of::<Matches>(Config::new())),
                )
                .field(
                    Field::plugins::<ConfiguredKind>("tie_breakers", "Tie breakers")
                        .with_default(json!([])),
                )
                .field(
                    Field::plugin::<ConfiguredKind>("fallback", "Fall back to")
                        .optional()
                        .with_default(Value::Null),
                )
        }
    }

    #[typetag::serde(name = "ranker")]
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

    /// The entry of the plugin `P`.
    fn entry_of<P: Plugin>() -> &'static PluginEntry {
        ConfiguredKind::entry(P::id()).expect("the plugin is registered")
    }

    /// The summary of the plugin `P` as `config` configures it.
    fn built<P: Plugin>(config: Config) -> Result<String, ConfigError> {
        PluginConfig::of::<P>(config)
            .build::<ConfiguredKind>()
            .map(|plugin| plugin.summary())
    }

    #[test]
    fn a_field_taking_plugins_knows_the_kind_it_takes() {
        let schema = <Ranker as Plugin>::schema();
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
        assert_eq!(
            entry_of::<Ranker>().sub_kinds(),
            [Kind::of::<ConfiguredKind>(); 3]
        );
        assert!(entry_of::<Points>().sub_kinds().is_empty());
    }

    #[test]
    fn a_default_survives_the_field_it_is_given_to_becoming_optional_or_repeated() {
        let optional = Field::decimal("for_bye", "Points for a bye")
            .with_default(0.5)
            .optional();
        let repeated = Field::decimal("weights", "Weight per round")
            .with_default(0.5)
            .repeated();

        assert_eq!(optional.default_value(), Some(&json!(0.5)));
        assert_eq!(repeated.default_value(), Some(&json!([0.5])));
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
                PluginConfig::of::<Points>(Config::new().with("for_win", 2.0)),
            )
            .with(
                "tie_breakers",
                vec![
                    PluginConfig::of::<Matches>(Config::new()),
                    PluginConfig::of::<Threshold>(Config::new().with("least", 4)),
                ],
            )
            .with("fallback", PluginConfig::of::<Matches>(Config::new()));

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
            PluginConfig::of::<Points>(Config::new().with("for_win", -1.0)),
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
        let config = Config::new().with("by", PluginConfig::new("unregistered", Config::new()));

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
            .with(
                "by",
                PluginConfig::of::<Points>(Config::new().with("for_win", 2.0)),
            )
            .with("fallback", PluginConfig::of::<Matches>(Config::new()));
        let completed = <Ranker as Plugin>::schema().complete(&config).unwrap();
        let built = PluginConfig::of::<Ranker>(config)
            .build::<ConfiguredKind>()
            .unwrap();

        assert_eq!(
            completed.get("fallback"),
            Some(&json!({"type": "matches", "value": null}))
        );
        assert_eq!(
            serde_json::to_value(&built).ok(),
            Some(PluginConfig::of::<Ranker>(completed).into())
        );
    }

    #[test]
    fn a_plugin_without_parameters_is_completed_as_a_unit_struct_is_written() {
        assert_eq!(
            entry_of::<Matches>().complete(&Config::new()),
            Ok(Value::Null)
        );
        assert_eq!(
            entry_of::<Threshold>().complete(&Config::new().with("least", 2)),
            Ok(json!({"least": 2, "weights": []}))
        );
    }

    #[test]
    fn the_plugins_of_the_axis_serialize_as_they_are_built() {
        assert_eq!(check_serialization::<ConfiguredKind>(), Ok(()));
    }

    #[test]
    fn a_plugin_recommends_what_a_field_of_that_kind_offers() {
        let points = Points {
            for_win: 1.0,
            rounds: 3,
            counts_byes: true,
            for_bye: None,
        };

        assert_eq!(
            points.recommends(),
            vec![PluginConfig::of::<Matches>(Config::new())]
        );
        assert!(Matches.recommends().is_empty());
    }
}
