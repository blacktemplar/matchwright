//! Plugins and a composite derived rather than written by hand, on an axis of their own.

use std::fmt::Debug;

use plugin_factory::{
    Plugin, PluginType, axis,
    catalog::{AnyAxis, PluginEntry},
    compatibility::{Declaration, Incompatibility, check},
    config::{ConfigError, Path, PluginConfig, Value},
    draft::Draft,
    register,
    schema::{Configurable, Field, FieldKind, FieldType},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
use serde_json::json;

/// A plugin axis whose plugins report what they are configured with, so that a built plugin can
/// be checked without being downcast.
#[axis]
trait Derived: Plugin {}

/// A fact every [`Points`] provides.
#[derive(Debug)]
struct Scored;

/// A fact [`Points`] provides if it counts byes.
#[derive(Debug)]
struct ByesCounted;

/// A fact [`Points`] requires.
#[derive(Debug)]
struct Matches;

/// A number of points that cannot be negative, a field type of its own.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f64")]
struct NonNegative(f64);

impl TryFrom<f64> for NonNegative {
    type Error = &'static str;

    fn try_from(points: f64) -> Result<Self, Self::Error> {
        if points < 0.0 {
            return Err("the points cannot be negative");
        }
        Ok(Self(points))
    }
}

impl FieldType for NonNegative {
    fn kind() -> FieldKind {
        FieldKind::Decimal
    }
}

/// A plugin with a field of every way to get a default and one without.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "derive::points",
    label = "points",
    description = "points per result",
    declare_static = Self::declare_static,
    declare = Self::declare,
    recommends = Self::recommends
)]
struct Points {
    #[serde(default = "one")]
    #[plugin(
        label = "Points for a win",
        description = "what winning a match is worth"
    )]
    for_win: NonNegative,
    #[serde(rename = "byes")]
    #[plugin(label = "Count byes")]
    counts_byes: bool,
    #[serde(default)]
    #[plugin(label = "Rounds")]
    rounds: u32,
    #[serde(default)]
    #[plugin(label = "Points for a bye")]
    for_bye: Option<f64>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[plugin(label = "Cap")]
    cap: Option<i64>,
    #[serde(with = "percent", default = "half")]
    #[plugin(label = "Share in percent")]
    share: f64,
    #[serde(skip)]
    #[allow(dead_code)]
    cache: Vec<String>,
}

fn one() -> NonNegative {
    NonNegative(1.0)
}

fn half() -> f64 {
    0.5
}

/// A share written as a percentage.
mod percent {
    use super::*;

    pub fn serialize<S: Serializer>(share: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        (share * 100.0).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let percent = f64::deserialize(deserializer)?;
        if !(0.0..=100.0).contains(&percent) {
            return Err(D::Error::custom("a share is between 0 and 100 percent"));
        }
        Ok(percent / 100.0)
    }
}

// named as the functions of the traits they are forwarded to, which they take precedence over
impl Points {
    fn declare_static(declaration: &mut Declaration) {
        declaration
            .provide(Scored)
            .require::<Matches>("the matches");
    }

    fn declare(&self, declaration: &mut Declaration) {
        if self.counts_byes {
            declaration.provide(ByesCounted);
        }
    }

    fn recommends(&self) -> Vec<PluginConfig> {
        vec![PluginConfig::of::<Unit>()]
    }
}

#[register]
impl Derived for Points {}

/// A plugin without parameters or description.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(id = "derive::unit", label = "unit")]
struct Unit;

#[register]
impl Derived for Unit {}

/// A plugin made of other plugins, whose defaults come from its `Default` impl.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[serde(default)]
#[plugin(
    id = "derive::ranker",
    label = "ranker",
    description = "a plugin made of plugins"
)]
struct Ranker {
    #[plugin(label = "Rank by")]
    by: Box<dyn Derived>,
    #[plugin(label = "Tie breakers")]
    tie_breakers: Vec<Box<dyn Derived>>,
    #[plugin(label = "Fall back to")]
    fallback: Option<Box<dyn Derived>>,
}

impl Default for Ranker {
    fn default() -> Self {
        Self {
            by: Box::new(Unit),
            tie_breakers: Vec::new(),
            fallback: None,
        }
    }
}

#[register]
impl Derived for Ranker {}

/// A plugin keeping state it builds up once it is configured, which is no part of its schema.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(id = "derive::counter", label = "counter")]
struct Counter {
    #[plugin(label = "Limit")]
    limit: u32,
    #[serde(default)]
    #[plugin(skip)]
    seen: u32,
}

#[register]
impl Derived for Counter {}

/// A composite, configured as a plugin is but not one itself.
#[derive(Debug, Serialize, Deserialize, Configurable)]
struct Composite {
    #[plugin(label = "Main", description = "what the composite is built around")]
    main: Box<dyn Derived>,
    #[serde(default)]
    #[plugin(label = "Others")]
    others: Vec<Box<dyn Derived>>,
    #[serde(rename = "limit", deserialize_with = "Option::deserialize")]
    #[plugin(label = "Limit")]
    cap: Option<u32>,
}

/// The entry of the plugin `P`.
fn entry_of<P: PluginType>() -> &'static PluginEntry {
    AnyAxis::of::<dyn Derived>()
        .entry(P::ID)
        .expect("the plugin is registered")
}

/// The field `name` of the schema of the plugin `P`.
fn field_of<P: PluginType>(name: &str) -> Field {
    entry_of::<P>()
        .schema()
        .field_named(name)
        .cloned()
        .expect("the schema has the field")
}

/// [`Points`] with every field answered, by its default if it has one.
fn points() -> PluginConfig {
    PluginConfig::of::<Points>()
        .with("for_win", 1.0)
        .with("byes", true)
        .with("rounds", 0)
        .with("for_bye", Value::Null)
        .with("cap", Value::Null)
        .with("share", 50.0)
}

#[test]
fn a_derived_plugin_is_registered_under_its_id() {
    let entry = entry_of::<Points>();

    assert_eq!(Points::ID, "derive::points");
    assert_eq!(Points::LABEL, "points");
    assert_eq!(entry.label(), "points");
    assert_eq!(entry.description(), Some("points per result"));
    assert_eq!(entry_of::<Unit>().description(), None);
    assert_eq!(AnyAxis::of::<dyn Derived>().entries().count(), 4);
}

#[test]
fn a_skipped_field_is_no_part_of_the_schema_but_of_the_plugin() {
    let schema = entry_of::<Counter>().schema();
    let built = PluginConfig::of::<Counter>()
        .with("limit", 3)
        .build::<dyn Derived>()
        .unwrap();
    let saved = json!({"type": "derive::counter", "value": {"limit": 3, "seen": 2}});

    assert_eq!(
        schema.fields().iter().map(Field::name).collect::<Vec<_>>(),
        ["limit"]
    );
    // built from a config, it holds its default; saved, it holds what it had built up
    assert_eq!(format!("{built:?}"), "Counter { limit: 3, seen: 0 }");
    assert_eq!(
        format!(
            "{:?}",
            serde_json::from_value::<Box<dyn Derived>>(saved).unwrap()
        ),
        "Counter { limit: 3, seen: 2 }"
    );
}

#[test]
fn an_answer_to_a_skipped_field_is_dropped() {
    let built = PluginConfig::of::<Counter>()
        .with("limit", 3)
        .with("seen", 2)
        .build::<dyn Derived>();

    assert_eq!(format!("{built:?}"), "Ok(Counter { limit: 3, seen: 0 })");
    assert_eq!(
        PluginConfig::of::<Counter>()
            .with("limit", 3)
            .with("unknown", 2)
            .build::<dyn Derived>()
            .err(),
        Some(ConfigError::UnknownField {
            path: Path::ROOT.field("unknown"),
        })
    );
}

#[test]
fn a_built_composite_with_state_is_drafted_without_it() {
    let composite = Composite {
        main: Box::new(Counter { limit: 3, seen: 2 }),
        others: Vec::new(),
        cap: None,
    };

    let draft = Draft::from_built(&composite).unwrap();

    assert_eq!(
        Value::from(draft.answers().clone()),
        json!({
            "main": {"type": "derive::counter", "value": {"limit": 3}},
            "others": [],
            "limit": null,
        })
    );
}

#[test]
fn a_built_plugin_is_tagged_with_its_id() {
    let built = PluginConfig::of::<Unit>().build::<dyn Derived>().unwrap();

    assert_eq!(
        serde_json::to_value(&built).unwrap(),
        json!({"type": "derive::unit", "value": null})
    );
}

#[test]
fn the_schema_has_the_fields_serde_deserializes_in_their_order() {
    let schema = entry_of::<Points>().schema();
    let names: Vec<_> = schema.fields().iter().map(Field::name).collect();

    assert_eq!(
        names,
        ["for_win", "byes", "rounds", "for_bye", "cap", "share"]
    );
    assert!(entry_of::<Unit>().schema().fields().is_empty());
}

#[test]
fn a_field_is_described_by_its_attribute() {
    let field = field_of::<Points>("for_win");

    assert_eq!(field.label(), "Points for a win");
    assert_eq!(field.description(), Some("what winning a match is worth"));
    assert_eq!(field_of::<Points>("byes").description(), None);
}

#[test]
fn a_field_takes_the_kind_of_answer_its_type_deserializes_from() {
    let plugin = FieldKind::plugin::<dyn Derived>();

    assert_eq!(field_of::<Points>("for_win").kind(), &FieldKind::Decimal);
    assert_eq!(field_of::<Points>("byes").kind(), &FieldKind::Bool);
    assert_eq!(field_of::<Points>("rounds").kind(), &FieldKind::Integer);
    assert_eq!(
        field_of::<Points>("for_bye").kind(),
        &FieldKind::optional(FieldKind::Decimal)
    );
    assert_eq!(field_of::<Ranker>("by").kind(), &plugin);
    assert_eq!(
        field_of::<Ranker>("tie_breakers").kind(),
        &FieldKind::list(plugin.clone())
    );
    assert_eq!(
        field_of::<Ranker>("fallback").kind(),
        &FieldKind::optional(plugin)
    );
    assert_eq!(
        field_of::<Ranker>("tie_breakers").kind().axis(),
        Some(AnyAxis::of::<dyn Derived>())
    );
}

#[test]
fn a_field_defaults_to_what_serde_fills_it_with() {
    let default = |name| field_of::<Points>(name).default_value().cloned();

    assert_eq!(default("for_win"), Some(json!(1.0)));
    assert_eq!(default("byes"), None);
    assert_eq!(default("rounds"), Some(json!(0)));
    assert_eq!(default("for_bye"), Some(Value::Null));
    // an option offers nothing only by a default of its own
    assert_eq!(default("cap"), None);
    // written as the field is serialized
    assert_eq!(default("share"), Some(json!(50.0)));
}

#[test]
fn a_struct_default_is_the_default_of_every_field() {
    let default = |name| field_of::<Ranker>(name).default_value().cloned();

    assert_eq!(
        default("by"),
        Some(json!({"type": "derive::unit", "value": null}))
    );
    assert_eq!(default("tie_breakers"), Some(json!([])));
    assert_eq!(default("fallback"), Some(Value::Null));
}

#[test]
fn a_derived_plugin_is_built_from_its_answers() {
    let built = points().with("share", 20).build::<dyn Derived>().unwrap();

    assert_eq!(
        serde_json::to_value(&built).unwrap(),
        json!({"type": "derive::points", "value": {
            "for_win": 1.0,
            "byes": true,
            "rounds": 0,
            "for_bye": null,
            "cap": null,
            "share": 20.0,
        }})
    );
}

#[test]
fn an_answer_the_field_type_rejects_is_rejected() {
    assert_eq!(
        points().with("for_win", -1.0).build::<dyn Derived>().err(),
        Some(ConfigError::Rejected {
            path: Path::ROOT.field("for_win"),
            reason: "the points cannot be negative".to_owned(),
        })
    );
}

#[test]
fn the_declaration_is_forwarded_to_the_functions_given() {
    let built = |points: PluginConfig| points.build::<dyn Derived>().unwrap();
    let counted = built(points());
    let uncounted = built(points().with("byes", false));
    let matches = |declaration: &mut Declaration| {
        declaration.provide(Matches);
    };

    // the static declaration requires the matches and provides a score
    assert_eq!(
        check(|_| {}, &[&*uncounted]),
        Err(Incompatibility::FactNotProvided {
            fact: "the matches",
            required_by: format!("{uncounted:?}"),
        })
    );
    assert_eq!(check(matches, &[&*uncounted]), Ok(()));
    assert_eq!(
        check(with(Scored), &[&*uncounted]),
        Err(Incompatibility::FactProvidedByContext {
            fact: "Scored".to_owned(),
            provider: format!("{uncounted:?}"),
        })
    );
    // the configured one provides the counted byes if they are counted
    assert_eq!(check(with(ByesCounted), &[&*uncounted]), Ok(()));
    assert_eq!(
        check(with(ByesCounted), &[&*counted]),
        Err(Incompatibility::FactProvidedByContext {
            fact: "ByesCounted".to_owned(),
            provider: format!("{counted:?}"),
        })
    );
}

/// A context providing the matches [`Points`] requires and `fact`.
fn with<T: Debug + Sync + 'static>(fact: T) -> impl FnOnce(&mut Declaration) {
    |declaration| {
        declaration.provide(Matches).provide(fact);
    }
}

#[test]
fn the_plugins_in_the_fields_are_the_plugins_it_holds() {
    let points = json!({"type": "derive::points", "value": {"byes": true, "cap": null}});
    let ranker: Ranker = serde_json::from_value(json!({
        "by": points,
        "tie_breakers": [{"type": "derive::unit", "value": null}],
    }))
    .unwrap();

    assert_eq!(
        format!("{:?}", ranker.plugins()),
        format!("[{:?}, {:?}]", ranker.by, Unit)
    );
    assert!(Unit.plugins().is_empty());
    // what they declare is part of what it declares
    assert_eq!(
        check(with(ByesCounted), &[&ranker]),
        Err(Incompatibility::FactProvidedByContext {
            fact: "ByesCounted".to_owned(),
            provider: format!("{ranker:?}"),
        })
    );
}

#[test]
fn a_recommendation_is_forwarded_to_the_function_given() {
    let built = points().build::<dyn Derived>().unwrap();

    assert_eq!(built.recommends(), [PluginConfig::of::<Unit>()]);
    assert!(Unit.recommends().is_empty());
}

#[test]
fn a_composite_reads_its_schema_from_its_fields_as_a_plugin_does() {
    let schema = Composite::schema();
    let field = |name| schema.field_named(name).expect("the schema has the field");
    let plugin = FieldKind::plugin::<dyn Derived>();

    assert_eq!(
        schema.fields().iter().map(Field::name).collect::<Vec<_>>(),
        ["main", "others", "limit"]
    );
    assert_eq!(field("main").kind(), &plugin);
    assert_eq!(
        field("main").description(),
        Some("what the composite is built around")
    );
    assert_eq!(field("main").default_value(), None);
    assert_eq!(field("others").kind(), &FieldKind::list(plugin));
    assert_eq!(field("others").default_value(), Some(&json!([])));
    assert_eq!(
        field("limit").kind(),
        &FieldKind::optional(FieldKind::Integer)
    );
    assert_eq!(field("limit").default_value(), None);
}

#[test]
fn an_option_without_a_default_is_not_read_missing_as_nothing() {
    let composite = |limit: Option<Value>| {
        let mut config = json!({"main": {"type": "derive::unit", "value": null}});
        if let Some(limit) = limit {
            config["limit"] = limit;
        }
        serde_json::from_value::<Composite>(config).map(|composite| composite.cap)
    };

    assert_eq!(composite(Some(Value::Null)).ok(), Some(None));
    assert_eq!(composite(Some(json!(3))).ok(), Some(Some(3)));
    assert_eq!(
        composite(None).map_err(|error| error.to_string()),
        Err("missing field `limit`".to_owned())
    );
}

#[test]
fn the_plugins_of_a_composite_are_the_plugins_in_its_fields() {
    let composite: Composite = serde_json::from_value(json!({
        "main": {"type": "derive::unit", "value": null},
        "others": [{"type": "derive::points", "value": {"byes": true, "cap": null}}],
        "limit": null,
    }))
    .unwrap();

    assert_eq!(
        format!("{:?}", composite.plugins()),
        format!("[{:?}, {:?}]", Unit, composite.others[0])
    );
    assert_eq!(composite.cap, None);
}
