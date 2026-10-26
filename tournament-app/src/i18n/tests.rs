use std::{collections::BTreeSet, fmt::Display};

use plugin_factory::{
    PluginType as _,
    catalog::{AnyAxis, PluginEntry},
    schema::{Configurable as _, Schema},
};
use tournament_engine::{
    PhasePlugins,
    plugins::{
        metrics_calculator::MetricsCalculator,
        phase_format::{
            DoubleElimination, DoubleEliminationRound, KnockoutRound, PhaseFormat,
            SingleElimination, Stage,
        },
        ranker::SliceRanker,
    },
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use super::*;
use crate::error::{
    AddEntrantError, AddPhaseError, NoPhase, NothingToUndo, ReportEnteredError, ReportError,
    StartError, UndoReportError,
};

#[test]
fn a_language_is_found_by_its_tag_whatever_the_region() {
    assert_eq!(Language::from_tag("de-AT"), Some(Language::German));
    assert_eq!(Language::from_tag("EN"), Some(Language::English));
    assert_eq!(Language::from_tag("fr"), None);
}

#[test]
fn the_first_language_spoken_of_those_preferred_is_chosen() {
    assert_eq!(
        Language::negotiate(["fr-FR", "de-CH", "en-US"]),
        Language::German
    );
    assert_eq!(Language::negotiate(["fr-FR"]), Language::English);
    assert_eq!(Language::negotiate([]), Language::English);
}

#[test]
fn a_text_is_said_in_the_language_with_its_variables() {
    let translator = Translator::new(Language::German, &[]);

    assert_eq!(
        translator.text(&Text::new("error-duplicate-name").with("name", "Alice")),
        "es gibt schon einen Teilnehmer namens „\u{2068}Alice\u{2069}“",
        "a value is isolated from the text around it"
    );
    assert_eq!(
        translator.text(&Text::new("error-log-entry-unreadable").with("position", 3_usize)),
        "Eintrag \u{2068}3\u{2069} des Protokolls kann nicht gelesen werden: \
         \u{2068}{$reason}\u{2069}",
        "a variable not set shows as such"
    );
}

#[test]
fn a_message_missing_in_the_language_is_said_in_english() {
    let resources: Resources = |language| match language {
        Language::English => "greeting = hello",
        Language::German => "",
    };

    let translator = Translator::new(Language::German, &[resources]);

    assert_eq!(translator.get("greeting"), "hello");
    assert_eq!(
        translator.get("unknown"),
        "unknown",
        "a key nowhere is shown"
    );
}

#[test]
fn the_messages_of_a_consumer_come_after_those_of_the_app() {
    let resources: Resources = |language| match language {
        Language::English => "greeting = hello",
        Language::German => "greeting = hallo",
    };

    let translator = Translator::new(Language::German, &[resources]);

    assert_eq!(translator.get("greeting"), "hallo");
    assert_eq!(
        translator.get("error-no-phase"),
        "das Turnier hat noch keine Phase"
    );
}

#[test]
fn an_error_is_said_in_the_language() {
    let translator = Translator::new(Language::German, &[]);

    assert_eq!(
        translator.message(&ReportError::DrawNotAllowed),
        "ein Spiel dieses Formats kann nicht unentschieden enden"
    );
}

#[test]
fn an_error_says_in_english_what_its_display_says() {
    /// What `error` says through its message in English, without the marks isolating its values,
    /// and through its `Display`.
    fn both(error: impl Message + Display) -> (String, String) {
        (
            Translator::new(Language::English, &[])
                .message(&error)
                .replace(['\u{2068}', '\u{2069}'], ""),
            error.to_string(),
        )
    }

    for (message, display) in [
        both(AddEntrantError::EmptyName),
        both(AddEntrantError::DuplicateName("Alice".to_owned())),
        both(NoPhase),
        both(AddPhaseError::Invalid(Vec::new())),
        both(StartError::TooFewEntrants),
        both(ReportError::NotStarted),
        both(ReportEnteredError::Unreadable),
        both(ReportEnteredError::NoMatch(3)),
        both(UndoReportError::DependedOn),
        both(NothingToUndo),
    ] {
        assert_eq!(message, display);
    }
}

#[test]
fn the_messages_of_the_app_are_in_every_language() {
    check_languages(|language| language.resources()[0]).unwrap();
}

/// The app's messages come first, and a key given twice keeps its first message.
#[test]
fn a_check_finds_messages_of_the_app() {
    let resources: Resources = |_| "phase = Phase\n";

    assert_eq!(
        check(resources).unwrap_err(),
        [
            "en: phase is a message of the app",
            "de: phase is a message of the app"
        ]
    );
}

#[test]
fn a_check_finds_messages_missing_given_twice_or_with_other_attributes() {
    let resources: Resources = |language| match language {
        Language::English => "a = a\nb = b\nc =\n    .label = c\n",
        Language::German => "a = a\na = a\nc = c\nd = d\n",
    };

    assert_eq!(
        check(resources).unwrap_err(),
        [
            "de: a is given twice",
            "de: b is missing",
            r#"de: c has the attributes {} rather than {"label"}"#,
            "de: d is not in English",
        ]
    );
}

#[test]
fn a_plugin_is_said_in_the_language_or_else_as_its_code_says() {
    let german = Translator::new(Language::German, &[]);

    assert_eq!(
        german.plugin(
            "tournament_engine::round_robin",
            PluginText::Label,
            "round robin"
        ),
        "Jeder gegen jeden"
    );
    assert_eq!(
        german.plugin(
            "tournament_engine::points",
            PluginText::Field("points_for_bye"),
            "points for a bye"
        ),
        "Punkte für ein Freilos"
    );
    assert_eq!(
        german.plugin("my_crate::swiss", PluginText::Label, "Swiss"),
        "Swiss"
    );
    assert_eq!(
        Translator::new(Language::English, &[]).plugin(
            "tournament_engine::round_robin",
            PluginText::Label,
            "round robin"
        ),
        "round robin"
    );
}

#[test]
fn a_stage_is_said_in_the_language_with_its_numbers_or_else_as_its_code_says() {
    let german = Translator::new(Language::German, &[]);
    let stage = |format, stage| StageName { format, stage };

    assert_eq!(
        german.stage(&stage(
            SingleElimination::ID,
            KnockoutRound::RoundOf(16).into()
        )),
        "Achtelfinale"
    );
    assert_eq!(
        german.stage(&stage(
            DoubleElimination::ID,
            DoubleEliminationRound::Losers {
                round: 3,
                rounds: 4
            }
            .into()
        )),
        "Verliererrunde \u{2068}3\u{2069}"
    );
    assert_eq!(
        german.stage(&stage("my_crate::swiss", Stage::new("playoff", "Playoff"))),
        "Playoff"
    );
    assert_eq!(
        Translator::new(Language::English, &[]).stage(&stage(
            SingleElimination::ID,
            KnockoutRound::RoundOf(16).into()
        )),
        "8th-finals"
    );
}

/// The plugins of the engine, of every axis, except those only its tests use.
fn engine_plugins() -> Vec<&'static PluginEntry> {
    [
        AnyAxis::of::<dyn PhaseFormat>(),
        AnyAxis::of::<dyn SliceRanker>(),
        AnyAxis::of::<dyn MetricsCalculator>(),
    ]
    .iter()
    .flat_map(AnyAxis::entries)
    .filter(|entry| {
        entry.id().starts_with("tournament_engine::") && !entry.id().contains("::test_utils::")
    })
    .collect()
}

/// What is translated of `entry`, as the attributes of its message.
fn translatable(entry: &PluginEntry) -> BTreeSet<String> {
    let mut attributes = BTreeSet::from([PluginText::Label.attribute()]);
    if entry.description().is_some() {
        attributes.insert(PluginText::Description.attribute());
    }
    attributes.extend(translatable_fields(&entry.schema()));
    attributes.extend(
        stages(entry.id())
            .iter()
            .map(|stage| PluginText::Stage(stage.key()).attribute()),
    );
    attributes
}

/// A stage of every key the format `id` names, none for a plugin that is no such format. The
/// stages cannot be found from the plugin, so the formats naming them are listed here.
fn stages(id: &str) -> Vec<Stage> {
    if id == SingleElimination::ID {
        [
            KnockoutRound::RoundOf(2),
            KnockoutRound::RoundOf(4),
            KnockoutRound::RoundOf(8),
            KnockoutRound::RoundOf(16),
            KnockoutRound::ThirdPlace,
        ]
        .map(Stage::from)
        .into()
    } else if id == DoubleElimination::ID {
        [
            DoubleEliminationRound::Winners {
                round: 1,
                rounds: 2,
            },
            DoubleEliminationRound::Winners {
                round: 2,
                rounds: 2,
            },
            DoubleEliminationRound::Losers {
                round: 1,
                rounds: 2,
            },
            DoubleEliminationRound::Losers {
                round: 2,
                rounds: 2,
            },
            DoubleEliminationRound::GrandFinal,
            DoubleEliminationRound::GrandFinalReset,
        ]
        .map(Stage::from)
        .into()
    } else {
        Vec::new()
    }
}

/// What is translated of the fields of `schema`, as attributes.
fn translatable_fields(schema: &Schema) -> BTreeSet<String> {
    let mut attributes = BTreeSet::new();
    for field in schema.fields() {
        attributes.insert(PluginText::Field(field.name()).attribute());
        if field.description().is_some() {
            attributes.insert(PluginText::FieldDescription(field.name()).attribute());
        }
    }
    attributes
}

/// The fields of a phase itself are translated as well, in the message [`PHASE`].
#[test]
fn every_plugin_of_the_engine_is_translated_and_nothing_else() {
    let calculators: Vec<_> = AnyAxis::of::<dyn MetricsCalculator>()
        .entries()
        .map(PluginEntry::id)
        .collect();
    let expected: BTreeMap<String, BTreeSet<String>> = engine_plugins()
        .into_iter()
        .map(|entry| (PluginText::key(entry.id()), translatable(entry)))
        .chain([(
            PHASE.to_owned(),
            translatable_fields(&PhasePlugins::schema()),
        )])
        .collect();
    for language in Language::ALL {
        if language == Language::English {
            continue;
        }
        let resource = parser::parse(language.resources()[1]).unwrap();
        let mut translated = BTreeMap::new();
        for entry in resource.body {
            if let ast::Entry::Message(message) = entry {
                let mut attributes: BTreeSet<_> = message
                    .attributes
                    .iter()
                    .map(|attribute| attribute.id.name.to_owned())
                    .collect();
                let key = message.id.name.to_owned();
                // only a calculator has a column, and not every calculator: the column cannot be
                // checked without the plugin
                if calculators.iter().any(|id| PluginText::key(id) == key) {
                    attributes.remove(&PluginText::Column.attribute());
                }
                assert!(
                    translated.insert(key.clone(), attributes).is_none(),
                    "{key} is given twice"
                );
            }
        }
        assert_eq!(translated, expected, "{}", language.tag());
    }
}
