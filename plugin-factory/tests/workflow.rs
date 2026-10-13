//! The whole way of a composite through the crate, on derived plugins of two axes of its own: a
//! program browses the catalog, an operator configures a phase with the draft, the phase is
//! built, saved and loaded again, and what a phase built elsewhere lacks is resolved.

use plugin_factory::{
    Plugin, PluginType, axis,
    catalog::AnyAxis,
    compatibility::{Declaration, Fact, Incompatibility, check, order},
    config::{Config, ConfigError, Path, PluginConfig, Value},
    draft::{self, Draft, Issue, Problem},
    register,
    schema::{Configurable, FieldKind, Schema},
};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::json;

/// How many entrants a ranking stands for, which the context of a phase provides.
#[derive(Debug)]
struct EntrantsPerRanking(u32);

impl Fact for EntrantsPerRanking {}

/// A fact a format provides if an entrant can be out of the phase.
#[derive(Debug)]
struct Eliminating;

impl Fact for Eliminating {}

/// A fact a format provides if an entrant can sit out a round.
#[derive(Debug)]
struct Byes;

impl Fact for Byes {}

/// A fact a metric provides if it awards points.
#[derive(Debug)]
struct Points;

impl Fact for Points {}

/// A fact a metric provides if it counts the matches played.
#[derive(Debug)]
struct Matches;

impl Fact for Matches {}

/// How the matches of a phase are scheduled.
#[axis]
trait Format: Plugin {}

/// What is calculated for an entrant.
#[axis]
trait Metric: Plugin {}

/// A format recommending metrics, two of which provide the same fact.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "full::round_robin",
    label = "round robin",
    description = "everyone plays everyone",
    declare_static = Self::declare_static,
    recommends = Self::recommends
)]
struct RoundRobin {
    #[plugin(label = "Rounds", default = 1)]
    rounds: u32,
}

impl RoundRobin {
    fn declare_static(declaration: &mut Declaration) {
        declaration.provide(Byes);
    }

    fn recommends(&self) -> Vec<PluginConfig> {
        vec![plugin::<Wins>(), plugin::<MatchCount>(), plugin::<Score>()]
    }
}

#[register]
impl Format for RoundRobin {}

/// A format providing a fact no metric provides.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "full::knockout",
    label = "knockout",
    description = "the loser of a match is out",
    declare_static = Self::declare_static
)]
struct Knockout {
    #[plugin(label = "Play for third place", default)]
    third_place: bool,
}

impl Knockout {
    fn declare_static(declaration: &mut Declaration) {
        declaration.provide(Eliminating);
    }
}

#[register]
impl Format for Knockout {}

/// A format with a condition on the fact the context provides.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "full::doubles",
    label = "doubles",
    description = "pairs play pairs",
    declare_static = Self::declare_static
)]
struct Doubles;

impl Doubles {
    fn declare_static(declaration: &mut Declaration) {
        declaration.require_that::<EntrantsPerRanking>(
            "the entrants per ranking",
            "rankings of pairs",
            |entrants| entrants.0 == 2,
        );
    }
}

#[register]
impl Format for Doubles {}

/// A metric with a parameter it validates and one changing what it requires.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "full::wins",
    label = "wins",
    description = "points per win",
    declare_static = Self::declare_static,
    declare = Self::declare
)]
struct Wins {
    #[serde(deserialize_with = "non_negative")]
    #[plugin(label = "Points for a win", default = 1.0)]
    for_win: f64,
    #[plugin(
        label = "Count byes",
        description = "whether a bye counts as a win",
        default
    )]
    counts_byes: bool,
}

fn non_negative<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let points = f64::deserialize(deserializer)?;
    if points < 0.0 {
        return Err(D::Error::custom("the points for a win cannot be negative"));
    }
    Ok(points)
}

impl Wins {
    fn declare_static(declaration: &mut Declaration) {
        declaration.provide(Points);
    }

    fn declare(&self, declaration: &mut Declaration) {
        if self.counts_byes {
            declaration.require::<Byes>("byes");
        }
    }
}

#[register]
impl Metric for Wins {}

/// A metric providing what [`Wins`] provides.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "full::score",
    label = "score",
    description = "the points scored",
    declare_static = Self::declare_static
)]
struct Score;

impl Score {
    fn declare_static(declaration: &mut Declaration) {
        declaration.provide(Points);
    }
}

#[register]
impl Metric for Score {}

/// The only metric providing [`Matches`].
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "full::match_count",
    label = "match count",
    description = "the matches played",
    declare_static = Self::declare_static
)]
struct MatchCount;

impl MatchCount {
    fn declare_static(declaration: &mut Declaration) {
        declaration.provide(Matches);
    }
}

#[register]
impl Metric for MatchCount {}

/// A metric requiring what other metrics provide.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "full::buchholz",
    label = "buchholz",
    description = "the points of the opponents",
    declare_static = Self::declare_static
)]
struct Buchholz;

impl Buchholz {
    fn declare_static(declaration: &mut Declaration) {
        declaration
            .require::<Points>("points")
            .require::<Matches>("the matches");
    }
}

#[register]
impl Metric for Buchholz {}

/// A metric requiring what only a format provides.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "full::losses",
    label = "losses",
    description = "the matches lost",
    declare_static = Self::declare_static
)]
struct Losses;

impl Losses {
    fn declare_static(declaration: &mut Declaration) {
        declaration.require::<Eliminating>("eliminations");
    }
}

#[register]
impl Metric for Losses {}

/// A metric made of metrics, so that plugins nest.
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "full::combined",
    label = "combined",
    description = "several metrics as one"
)]
struct Combined {
    #[plugin(label = "Parts", default)]
    parts: Vec<Box<dyn Metric>>,
}

#[register]
impl Metric for Combined {}

/// The composite: one format, any number of metrics, and a metric or none.
#[derive(Debug, Serialize, Deserialize, Configurable)]
struct Phase {
    #[plugin(label = "Format", description = "how the matches are scheduled")]
    format: Box<dyn Format>,
    #[plugin(label = "Metrics", default)]
    metrics: Vec<Box<dyn Metric>>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[plugin(label = "Tie break", default)]
    tie_break: Option<Box<dyn Metric>>,
}

/// The context of a phase of single players.
fn singles(declaration: &mut Declaration) {
    declaration.provide(EntrantsPerRanking(1));
}

/// The path written as `text`, e.g. `metrics[1].for_win`.
fn path(text: &str) -> Path {
    text.parse().unwrap()
}

/// The plugin `P` with nothing answered.
fn plugin<P: PluginType>() -> PluginConfig {
    PluginConfig::of::<P>()
}

/// [`Wins`] with a point per win, counting byes if `counts_byes`.
fn wins(counts_byes: bool) -> PluginConfig {
    plugin::<Wins>()
        .with("for_win", 1.0)
        .with("counts_byes", counts_byes)
}

/// Answers every unanswered field of `draft` that offers an answer with it, in question order,
/// as a UI accepting every default does.
fn accept_defaults(draft: &mut Draft<Phase>) {
    loop {
        let next = questions(draft)
            .into_iter()
            .filter(|(_, answered)| !answered)
            .find_map(|(at, _)| {
                let at = path(&at);
                let default = draft::default_value(singles, draft, &at).unwrap()?;
                Some((at, default))
            });
        let Some((at, default)) = next else {
            return;
        };
        draft.set(&at, default).unwrap();
    }
}

/// The fields a UI renders for `draft`, by path, each with whether it is answered: the schema of
/// the phase walked along the answers, as documented on [`Draft`].
fn questions(draft: &Draft<Phase>) -> Vec<(String, bool)> {
    let mut questions = Vec::new();
    fields(
        &Phase::schema(),
        draft.answers(),
        &Path::ROOT,
        &mut questions,
    );
    questions
}

/// Every field of `schema`, the schema of the config at `at`, each followed by the fields of the
/// plugins answering it in `config`.
fn fields(schema: &Schema, config: &Config, at: &Path, questions: &mut Vec<(String, bool)>) {
    for field in schema.fields() {
        let path = at.clone().field(field.name());
        let answer = config.get(field.name());
        questions.push((path.to_string(), answer.is_some()));
        if let Some(answer) = answer {
            plugin_fields(field.kind(), answer, &path, questions);
        }
    }
}

/// The fields of the plugins in `answer`, an answer of `kind` at `at`.
fn plugin_fields(kind: &FieldKind, answer: &Value, at: &Path, questions: &mut Vec<(String, bool)>) {
    match (kind, answer) {
        (FieldKind::Optional(inner), _) if !answer.is_null() => {
            plugin_fields(inner, answer, at, questions);
        }
        (FieldKind::List { element, .. }, Value::Array(answers)) => {
            for (index, answer) in answers.iter().enumerate() {
                let path = at.clone().index(index);
                plugin_fields(element, answer, &path, questions);
            }
        }
        (FieldKind::Plugin(axis), _) => {
            let plugin = PluginConfig::deserialize(answer).unwrap();
            let schema = axis.entry(plugin.plugin()).unwrap().schema();
            fields(&schema, plugin.config(), at, questions);
        }
        _ => {}
    }
}

/// The issues, each as its path and problem.
fn problems(issues: &[Issue]) -> Vec<(String, &Problem)> {
    issues
        .iter()
        .map(|issue| (issue.path().to_string(), issue.problem()))
        .collect()
}

/// An issue about a fact nothing provides as its path, the fact, and the list and plugin of each
/// addition finishing makes for it.
type Unprovided = (String, &'static str, Vec<(String, &'static str)>);

/// The issues about a fact nothing provides, see [`Unprovided`].
///
/// # Panics
/// Panics if an issue is about something else.
fn unprovided(issues: &[Issue]) -> Vec<Unprovided> {
    issues
        .iter()
        .map(|issue| match issue.problem() {
            Problem::Unprovided {
                fact,
                added_by_finish,
                ..
            } => (
                issue.path().to_string(),
                *fact,
                added_by_finish
                    .iter()
                    .map(|addition| (addition.list().to_string(), addition.plugin()))
                    .collect(),
            ),
            problem => panic!("{problem:?} is not about a fact nothing provides"),
        })
        .collect()
}

/// The ids of the candidates at `at` that fit.
fn fitting(draft: &Draft<Phase>, at: &str) -> Vec<&'static str> {
    draft::candidates(singles, draft, &path(at))
        .unwrap()
        .iter()
        .filter(|candidate| candidate.conflicts().is_empty())
        .map(|candidate| candidate.entry().id())
        .collect()
}

/// Asserts that the plugin `id` is a candidate at `at` that does not fit because of `conflict`.
fn assert_conflict(draft: &Draft<Phase>, at: &str, id: &str, conflict: Incompatibility) {
    let candidates = draft::candidates(singles, draft, &path(at)).unwrap();
    let candidate = candidates
        .iter()
        .find(|candidate| candidate.entry().id() == id)
        .expect("the plugin is a candidate");
    let conflicts: Vec<_> = candidate
        .conflicts()
        .iter()
        .map(|conflict| conflict.incompatibility())
        .collect();
    assert_eq!(conflicts, [&conflict]);
}

/// The ids of the plugins of `axis`, which come sorted.
fn ids(axis: AnyAxis) -> Vec<&'static str> {
    axis.entries().map(|entry| entry.id()).collect()
}

#[test]
fn a_program_finds_every_plugin_through_the_schema_of_the_composite() {
    let axes: Vec<_> = Phase::schema()
        .fields()
        .iter()
        .map(|field| (field.name(), field.kind().axis().unwrap()))
        .collect();
    let metrics = [
        "full::buchholz",
        "full::combined",
        "full::losses",
        "full::match_count",
        "full::score",
        "full::wins",
    ];

    assert_eq!(
        axes.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        ["format", "metrics", "tie_break"]
    );
    assert_eq!(
        ids(axes[0].1),
        ["full::doubles", "full::knockout", "full::round_robin"]
    );
    assert_eq!(ids(axes[1].1), metrics);
    assert_eq!(ids(axes[2].1), metrics);
    let schema = Phase::schema();
    let default = |name| schema.field_named(name).unwrap().default_value();
    assert_eq!(
        schema.field_named("format").unwrap().description(),
        Some("how the matches are scheduled")
    );
    assert_eq!(default("format"), None);
    assert_eq!(default("metrics"), Some(&json!([])));
    assert_eq!(default("tie_break"), Some(&Value::Null));

    let wins = axes[1].1.entry("full::wins").unwrap();
    let schema = wins.schema();
    let counts_byes = schema.field_named("counts_byes").unwrap();
    assert_eq!(wins.label(), "wins");
    assert_eq!(wins.description(), Some("points per win"));
    assert_eq!(
        schema.field_named("for_win").unwrap().default_value(),
        Some(&json!(1.0))
    );
    assert_eq!(counts_byes.label(), "Count byes");
    assert_eq!(
        counts_byes.description(),
        Some("whether a bye counts as a win")
    );
    assert_eq!(counts_byes.default_value(), Some(&json!(false)));
    // a plugin with fields taking plugins leads on to their axes
    let combined = axes[1].1.entry("full::combined").unwrap().schema();
    assert_eq!(
        combined.fields()[0].kind().axis(),
        Some(AnyAxis::of::<dyn Metric>())
    );
}

#[test]
fn a_built_plugin_is_checked_by_what_its_configuration_declares() {
    let metric = |plugin: PluginConfig| plugin.build::<dyn Metric>().unwrap();
    let uncounting = metric(wins(false));
    let counting = metric(wins(true));
    let buchholz = metric(plugin::<Buchholz>());

    assert_eq!(
        check(singles, &[&*buchholz]),
        Err(Incompatibility::FactNotProvided {
            fact: "points",
            required_by: "Buchholz".to_owned(),
        })
    );
    // what a configuration adds counts as well
    assert_eq!(check(singles, &[&*uncounting]), Ok(()));
    assert_eq!(
        check(singles, &[&*counting]),
        Err(Incompatibility::FactNotProvided {
            fact: "byes",
            required_by: format!("{counting:?}"),
        })
    );
}

#[test]
fn an_operator_configures_a_phase_one_answer_at_a_time() {
    let mut draft = Draft::<Phase>::new();
    assert_eq!(
        questions(&draft),
        [
            ("format".to_owned(), false),
            ("metrics".to_owned(), false),
            ("tie_break".to_owned(), false),
        ]
    );

    // the format: a candidate conflicting with the context is offered with why
    assert_eq!(
        fitting(&draft, "format"),
        ["full::knockout", "full::round_robin"]
    );
    assert_conflict(
        &draft,
        "format",
        "full::doubles",
        Incompatibility::ConditionNotMet {
            condition: "rankings of pairs",
            required_by: "doubles".to_owned(),
            provided: "EntrantsPerRanking(1)".to_owned(),
        },
    );
    draft.set(&path("format"), plugin::<Knockout>()).unwrap();
    assert_eq!(
        questions(&draft),
        [
            ("format".to_owned(), true),
            ("format.third_place".to_owned(), false),
            ("metrics".to_owned(), false),
            ("tie_break".to_owned(), false),
        ]
    );
    assert_eq!(
        draft::default_value(singles, &draft, &path("format.third_place")),
        Ok(Some(json!(false)))
    );
    // the offered answer is only filled in by setting it
    draft.set(&path("format.third_place"), false).unwrap();
    // a knockout recommends nothing, so the schema's default is offered
    assert_eq!(
        draft::default_value(singles, &draft, &path("metrics")),
        Ok(Some(json!([])))
    );

    // metrics requiring facts the unanswered tie break could still provide are no issue yet
    draft
        .set(
            &path("metrics"),
            json!([plugin::<Losses>(), plugin::<Buchholz>()]),
        )
        .unwrap();
    assert_eq!(draft::issues(singles, &draft), []);

    // once it is answered, the requirements are reported at every field that could meet them
    draft.set(&path("tie_break"), Value::Null).unwrap();
    // both are added by finishing, so a UI may leave them out
    let scored = vec![("metrics".to_owned(), "full::score")];
    let counted = vec![("metrics".to_owned(), "full::match_count")];
    assert_eq!(
        unprovided(&draft::issues(singles, &draft)),
        [
            ("metrics".to_owned(), "points", scored.clone()),
            ("tie_break".to_owned(), "points", scored),
            ("metrics".to_owned(), "the matches", counted.clone()),
            ("tie_break".to_owned(), "the matches", counted.clone()),
        ]
    );

    // an answer the plugin rejects is set, reported where it is fixed and keeps the phase from
    // being built
    draft
        .set(&path("metrics[2]"), wins(false).with("for_win", -1))
        .unwrap();
    let rejected = || ConfigError::Rejected {
        path: path("metrics[2].for_win"),
        reason: "the points for a win cannot be negative".to_owned(),
    };
    let issues = draft::issues(singles, &draft);
    assert_eq!(issues[0].path(), &path("metrics[2].for_win"));
    assert_eq!(issues[0].problem(), &Problem::Invalid(rejected()));
    assert_eq!(
        unprovided(&issues[1..]),
        [
            ("metrics".to_owned(), "the matches", counted.clone()),
            ("tie_break".to_owned(), "the matches", counted),
        ]
    );
    // finishing settles the matches, which a single metric provides, but not the answer
    let issues = draft::finish(singles, &draft).unwrap_err();
    assert_eq!(
        problems(&issues),
        [(
            "metrics[2].for_win".to_owned(),
            &Problem::Invalid(rejected())
        )]
    );

    // a whole number answers a decimal field
    draft.set(&path("metrics[2].for_win"), 3).unwrap();

    // the candidates for one more metric are the ones not in the list yet fitting next to the
    // others; what they miss can still be added to the metrics
    assert_eq!(
        fitting(&draft, "metrics[3]"),
        ["full::combined", "full::match_count"]
    );
    assert_conflict(
        &draft,
        "metrics[3]",
        "full::score",
        Incompatibility::FactProvidedTwice {
            fact: "Points".to_owned(),
            first: "wins (metrics[2])".to_owned(),
            second: "score".to_owned(),
        },
    );

    // the match count is left to finishing, which appends it
    let phase = draft::finish(singles, &draft).unwrap();
    assert_eq!(
        serde_json::to_value(&phase).unwrap(),
        json!({
            "format": {"type": "full::knockout", "value": {"third_place": false}},
            "metrics": [
                {"type": "full::losses", "value": null},
                {"type": "full::buchholz", "value": null},
                {"type": "full::wins", "value": {"for_win": 3.0, "counts_byes": false}},
                {"type": "full::match_count", "value": null},
            ],
            "tie_break": null,
        })
    );
    assert_eq!(check(singles, &phase.plugins()), Ok(()));
    // the buchholz runs after the metrics it requires
    assert_eq!(order(&phase.plugins()), Ok(vec![0, 1, 3, 4, 2]));
}

#[test]
fn a_phase_left_to_its_defaults_takes_what_its_format_recommends() {
    let mut draft = Draft::<Phase>::new();
    draft.set(&path("format"), plugin::<RoundRobin>()).unwrap();

    // a format with a field still unanswered recommends nothing yet
    assert_eq!(
        draft::default_value(singles, &draft, &path("metrics")),
        Ok(Some(json!([])))
    );
    draft.set(&path("format.rounds"), 1).unwrap();

    // the score does not fit next to the wins recommended before it
    assert_eq!(
        draft::default_value(singles, &draft, &path("metrics")),
        Ok(Some(json!([plugin::<Wins>(), plugin::<MatchCount>()])))
    );
    assert_eq!(
        draft::default_value(singles, &draft, &path("tie_break")),
        Ok(Some(json!(plugin::<Wins>())))
    );

    // finishing fills in no default
    let unanswered: Vec<_> = draft::finish(singles, &draft)
        .unwrap_err()
        .iter()
        .map(|issue| issue.path().to_string())
        .collect();
    assert_eq!(unanswered, ["metrics", "tie_break"]);

    // accepted in question order, the tie break no longer fits the recommended metrics once
    // they are set
    accept_defaults(&mut draft);
    let phase = draft::finish(singles, &draft).unwrap();
    assert_eq!(
        serde_json::to_value(&phase).unwrap(),
        json!({
            "format": {"type": "full::round_robin", "value": {"rounds": 1}},
            "metrics": [
                {"type": "full::wins", "value": {"for_win": 1.0, "counts_byes": false}},
                {"type": "full::match_count", "value": null},
            ],
            "tie_break": null,
        })
    );
}

#[test]
fn a_requirement_of_a_configuration_is_reported_where_it_can_be_met() {
    let mut draft = Draft::<Phase>::new();
    draft
        .set(
            &path("format"),
            plugin::<Knockout>().with("third_place", false),
        )
        .unwrap();
    draft.set(&path("metrics"), json!([wins(true)])).unwrap();
    draft.set(&path("tie_break"), Value::Null).unwrap();

    let issues = draft::issues(singles, &draft);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path(), &path("format"));
    assert_eq!(
        issues[0].problem(),
        &Problem::Unprovided {
            fact: "byes",
            required_by: path("metrics[0]"),
            providers: vec!["full::round_robin"],
            added_by_finish: vec![],
        }
    );

    draft
        .set(&path("format"), plugin::<RoundRobin>().with("rounds", 1))
        .unwrap();
    assert_eq!(draft::issues(singles, &draft), []);
    assert!(draft::finish(singles, &draft).is_ok());
}

#[test]
fn a_saved_phase_loads_as_it_was_built() {
    let mut draft = Draft::<Phase>::new();
    draft.set(&path("format"), plugin::<RoundRobin>()).unwrap();
    draft
        .set(
            &path("tie_break"),
            plugin::<Combined>().with("parts", json!([plugin::<Buchholz>()])),
        )
        .unwrap();
    assert_eq!(
        questions(&draft),
        [
            ("format".to_owned(), true),
            ("format.rounds".to_owned(), false),
            ("metrics".to_owned(), false),
            ("tie_break".to_owned(), true),
            ("tie_break.parts".to_owned(), true),
        ]
    );
    accept_defaults(&mut draft);
    let phase = draft::finish(singles, &draft).unwrap();

    let saved = serde_json::to_string(&phase).unwrap();
    let loaded: Phase = serde_json::from_str(&saved).unwrap();

    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(&phase).unwrap()
    );
    let plugins = loaded.plugins();
    assert_eq!(check(singles, &plugins), Ok(()));
    // the buchholz nested in the tie break counts with its requirements
    assert_eq!(
        check(singles, &[plugins[0], plugins[3]]),
        Err(Incompatibility::FactNotProvided {
            fact: "points",
            required_by: format!("{:?}", plugins[3]),
        })
    );
    // a saved plugin is checked as an answer is
    let tampered = saved.replace(r#""for_win":1.0"#, r#""for_win":-1.0"#);
    assert_ne!(tampered, saved);
    assert!(serde_json::from_str::<Phase>(&tampered).is_err());
}

#[test]
fn what_a_built_phase_lacks_is_added_if_a_single_plugin_provides_it() {
    let metric = |plugin: PluginConfig| plugin.build::<dyn Metric>().unwrap();
    let phase = Phase {
        format: plugin::<Knockout>()
            .with("third_place", false)
            .build::<dyn Format>()
            .unwrap(),
        metrics: vec![metric(plugin::<Buchholz>())],
        tie_break: None,
    };

    // the wins have fields, so the score is the only provider of the points that is added, and
    // the match count the only one of the matches; the consumer then orders the metrics by what
    // they require
    let draft = Draft::from_built(&phase).unwrap();
    let phase = draft::finish(singles, &draft).unwrap();
    assert_eq!(
        format!("{:?}", phase.metrics),
        "[Buchholz, Score, MatchCount]"
    );
    assert_eq!(check(singles, &phase.plugins()), Ok(()));
    assert_eq!(order(&phase.plugins()), Ok(vec![0, 2, 3, 1]));
}
