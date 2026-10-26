use plugin_factory::PluginType;
use tournament_engine::plugins::{
    metrics_calculator::{
        BuchholzCalculator, FewerLosses, LaterFirstLoss, MoreBuchholz, MorePoints,
        MoreSonnebornBerger, MoreWins, PointsCalculator, RoundOfFirstLossCalculator,
        SonnebornBergerCalculator, WinsDrawsLossesCalculator,
    },
    phase_format::{MonsterDyp, RandomRounds, RoundRobin, SingleElimination},
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use super::*;
use crate::{
    i18n::{Language, Translator},
    session::Session,
};

/// The form of a new phase, as a session hands it out.
fn new_form() -> PhaseForm {
    Session::new("cup").phase_form().unwrap()
}

fn path(path: &str) -> Path {
    path.parse().unwrap()
}

/// The field at `path` of `view`.
#[track_caller]
fn field<'a>(view: &'a FormView, at: &str) -> &'a FormField {
    find(&view.fields, &path(at)).unwrap_or_else(|| panic!("no field at {at}"))
}

fn find<'a>(fields: &'a [FormField], path: &Path) -> Option<&'a FormField> {
    fields.iter().find_map(|field| {
        if field.path == *path {
            return Some(field);
        }
        match &field.input {
            Input::Plugin(PluginInput {
                chosen: Some(chosen),
                ..
            }) => find(&chosen.fields, path),
            Input::List(list) => list.items.iter().find_map(|item| find(&item.fields, path)),
            _ => None,
        }
    })
}

#[track_caller]
fn list<'a>(view: &'a FormView, path: &str) -> &'a ListInput {
    match &field(view, path).input {
        Input::List(list) => list,
        input => panic!("{path} takes no list but {input:?}"),
    }
}

#[track_caller]
fn plugin<'a>(view: &'a FormView, path: &str) -> &'a PluginInput {
    match &field(view, path).input {
        Input::Plugin(plugin) => plugin,
        input => panic!("{path} takes no plugin but {input:?}"),
    }
}

/// The ids of the plugins chosen in the list at `path`.
#[track_caller]
fn items(view: &FormView, path: &str) -> Vec<&'static str> {
    list(view, path)
        .items
        .iter()
        .map(|item| item.plugin.id)
        .collect()
}

#[track_caller]
fn chosen(view: &FormView, path: &str) -> &'static str {
    plugin(view, path).chosen.as_ref().unwrap().plugin.id
}

/// `text` in English, without the marks isolating its variables.
fn english(text: &Text) -> String {
    Translator::new(Language::English, &[])
        .text(text)
        .replace(['\u{2068}', '\u{2069}'], "")
}

fn english_all(texts: &[Text]) -> Vec<String> {
    texts.iter().map(english).collect()
}

/// A new phase is played in the format offered, with what the format recommends, and the form has
/// the fields of the phase, each with its answer, followed by those of the plugins chosen.
#[test]
fn a_new_phase_is_answered_as_its_format_recommends() {
    let view = new_form().view();

    let names: Vec<_> = view.fields.iter().map(|field| field.name).collect();
    assert_eq!(names, ["format", "ranking", "metrics"]);
    assert_eq!(chosen(&view, "format"), RoundRobin::ID);
    assert_eq!(
        items(&view, "ranking"),
        [MorePoints::ID, MoreSonnebornBerger::ID]
    );
    assert_eq!(
        items(&view, "metrics"),
        [
            WinsDrawsLossesCalculator::ID,
            PointsCalculator::ID,
            SonnebornBergerCalculator::ID
        ]
    );
    let win = field(&view, "metrics[1].points_for_win");
    assert_eq!(win.plugin, Some(PointsCalculator::ID));
    assert!(matches!(
        win.input,
        Input::Decimal {
            value: Some(1.0),
            ..
        }
    ));
    assert!(view.complete && view.issues.is_empty());
}

/// The plugins recommended come first, the others follow by label; a plugin chosen in a list
/// is not offered for it again.
#[test]
fn a_field_offers_the_plugins_that_fit_the_recommended_first() {
    let view = new_form().view();

    let formats: Vec<_> = plugin(&view, "format")
        .options
        .iter()
        .map(|option| option.label)
        .collect();
    assert_eq!(
        formats,
        [
            "round robin",
            "double elimination",
            "Monster DYP",
            "random rounds",
            "single elimination"
        ]
    );
    let ranking = list(&view, "ranking");
    assert!(ranking.items.iter().all(|item| item.plugin.recommended));
    assert!(
        ranking
            .options
            .iter()
            .all(|option| !option.recommended && option.id != MorePoints::ID)
    );
}

/// Choosing another format brings what it recommends, both its own fields and the ranking and
/// metrics; a field the operator changed stays as they changed it.
#[test]
fn the_fields_answered_as_offered_follow_another_format() {
    let mut form = new_form();
    form.choose(&path("format"), Some(SingleElimination::ID))
        .unwrap();
    let view = form.view();
    assert_eq!(
        items(&view, "ranking"),
        [LaterFirstLoss::ID, FewerLosses::ID]
    );
    assert_eq!(
        items(&view, "metrics"),
        [
            RoundOfFirstLossCalculator::ID,
            WinsDrawsLossesCalculator::ID
        ]
    );
    assert_eq!(
        field(&view, "format.third_place_match").input,
        Input::Bool(Some(true))
    );

    let mut form = new_form();
    form.add(&path("ranking"), MoreWins::ID).unwrap();
    form.choose(&path("format"), Some(SingleElimination::ID))
        .unwrap();

    let view = form.view();
    assert_eq!(
        items(&view, "ranking"),
        [MorePoints::ID, MoreSonnebornBerger::ID, MoreWins::ID]
    );
    assert_eq!(
        items(&view, "metrics"),
        [
            RoundOfFirstLossCalculator::ID,
            WinsDrawsLossesCalculator::ID
        ]
    );
}

/// What a plugin recommends follows its own fields too: a single elimination with a match for
/// third place recommends deciding it by the losses, one without it does not.
#[test]
fn the_fields_answered_as_offered_follow_a_field_of_a_plugin() {
    let mut form = new_form();
    form.choose(&path("format"), Some(SingleElimination::ID))
        .unwrap();

    form.set_bool(&path("format.third_place_match"), false)
        .unwrap();

    assert_eq!(items(&form.view(), "ranking"), [LaterFirstLoss::ID]);
}

#[test]
fn a_number_field_shows_the_numbers_it_takes() {
    let mut form = new_form();
    form.choose(&path("format"), Some(RandomRounds::ID))
        .unwrap();
    let view = form.view();

    let Input::Integer { value, hint, .. } = &field(&view, "format.rounds").input else {
        panic!("rounds are a whole number");
    };
    assert_eq!(*value, Some(5));
    assert_eq!(english(hint), "a whole number of at least 1");
    let Input::Decimal { hint, .. } = &field(&view, "metrics[1].points_for_win").input else {
        panic!("points are a number");
    };
    assert_eq!(english(hint), "a number of at least 0");
}

/// A number with a comma is taken as with a point, as operators write it in many languages.
#[test]
fn a_number_entered_is_taken_with_a_point_or_a_comma() {
    let mut form = new_form();

    form.enter(&path("metrics[1].points_for_win"), " 2,5 ")
        .unwrap();
    form.enter(&path("metrics[1].points_for_draw"), "1.5")
        .unwrap();

    let view = form.view();
    for (path, number) in [
        ("metrics[1].points_for_win", 2.5),
        ("metrics[1].points_for_draw", 1.5),
    ] {
        let Input::Decimal { value, entered, .. } = &field(&view, path).input else {
            panic!("points are a number");
        };
        assert_eq!((*value, entered.as_deref()), (Some(number), None));
    }
}

/// What a field does not take leaves its answer as it was, and the field shows what was entered
/// with why it was not taken.
#[test]
fn a_number_not_taken_is_shown_with_why() {
    let mut form = new_form();
    form.choose(&path("format"), Some(MonsterDyp::ID)).unwrap();

    form.enter(&path("format.rounds"), "0").unwrap();
    form.enter(&path("metrics[1].points_for_win"), "many")
        .unwrap();

    let view = form.view();
    let rounds = field(&view, "format.rounds");
    assert!(matches!(
        &rounds.input,
        Input::Integer { value: Some(5), entered: Some(entered), .. } if entered == "0"
    ));
    assert_eq!(
        english_all(&rounds.issues),
        ["this takes a whole number of at least 1"]
    );
    assert_eq!(
        english_all(&field(&view, "metrics[1].points_for_win").issues),
        ["“many” is no number"]
    );
    assert!(view.complete, "the answers stay as they were");

    form.enter(&path("format.rounds"), "7").unwrap();
    let view = form.view();
    assert!(field(&view, "format.rounds").issues.is_empty());
    assert!(matches!(
        field(&view, "format.rounds").input,
        Input::Integer {
            value: Some(7),
            entered: None,
            ..
        }
    ));
}

/// A metric a criterion needs is calculated without being chosen, if there is nothing to choose:
/// a single calculator without fields calculates it.
#[test]
fn a_calculator_needed_and_without_fields_is_implied() {
    let mut form = new_form();

    form.add(&path("ranking"), MoreBuchholz::ID).unwrap();

    let view = form.view();
    let metrics = list(&view, "metrics");
    let implied: Vec<_> = metrics.implied.iter().map(|option| option.id).collect();
    assert_eq!(implied, [BuchholzCalculator::ID]);
    assert!(
        metrics
            .options
            .iter()
            .all(|option| option.id != BuchholzCalculator::ID),
        "an implied plugin is not offered as well"
    );
    assert!(view.complete);
}

/// The ranking comes before the metrics, so removing the metric a criterion needs leaves the
/// criterion where it is, with an issue, rather than dropping it unnoticed.
#[test]
fn a_field_does_not_follow_a_change_of_a_field_after_it() {
    let mut form = new_form();

    form.remove(&path("metrics[2]")).unwrap();

    let view = form.view();
    assert_eq!(
        items(&view, "ranking"),
        [MorePoints::ID, MoreSonnebornBerger::ID]
    );
    assert_eq!(
        english_all(&field(&view, "metrics").issues),
        ["more Sonneborn-Berger needs one of these: Sonneborn-Berger"]
    );
}

/// The points can be scored in many ways, so the operator has to choose them, and the issue is
/// at the metrics, where it is fixed, naming what fixes it.
#[test]
fn an_issue_is_shown_at_the_field_that_fixes_it() {
    let mut form = new_form();

    form.remove(&path("metrics[1]")).unwrap();

    let view = form.view();
    assert!(!view.complete);
    assert_eq!(
        english_all(&field(&view, "metrics").issues),
        [
            "more points needs one of these: points",
            "Sonneborn-Berger needs one of these: points"
        ]
    );
    assert_eq!(
        Translator::new(Language::German, &[])
            .text(&field(&view, "metrics").issues[0])
            .replace(['\u{2068}', '\u{2069}'], ""),
        "mehr Punkte braucht eines davon: Punkte"
    );
    assert!(view.issues.is_empty());
}

#[test]
fn a_plugin_in_an_ordered_list_moves_up_and_down() {
    let mut form = new_form();
    form.add(&path("ranking"), MoreWins::ID).unwrap();

    form.move_up(&path("ranking[2]")).unwrap();
    assert_eq!(
        items(&form.view(), "ranking"),
        [MorePoints::ID, MoreWins::ID, MoreSonnebornBerger::ID]
    );
    form.move_down(&path("ranking[0]")).unwrap();
    assert_eq!(
        items(&form.view(), "ranking"),
        [MoreWins::ID, MorePoints::ID, MoreSonnebornBerger::ID]
    );

    form.move_up(&path("ranking[0]")).unwrap();
    form.move_down(&path("ranking[2]")).unwrap();
    assert_eq!(
        items(&form.view(), "ranking"),
        [MoreWins::ID, MorePoints::ID, MoreSonnebornBerger::ID],
        "the first and the last stay where they are"
    );
}

#[test]
fn a_field_that_takes_no_number_is_entered_nothing() {
    let mut form = new_form();

    assert!(form.enter(&path("format"), "3").is_err());
    assert!(form.enter(&path("rounds"), "3").is_err());
}

/// The phase is added as the form answered it, and an edit starts out at that.
#[test]
fn a_phase_is_added_and_edited_through_its_form() {
    let mut session = Session::new("cup");
    let mut form = session.phase_form().unwrap();
    form.choose(&path("format"), Some(MonsterDyp::ID)).unwrap();
    form.enter(&path("format.rounds"), "7").unwrap();

    session.add_phase(form.draft()).unwrap();

    let mut form = session.phase_form_to_edit().unwrap();
    let view = form.view();
    assert_eq!(chosen(&view, "format"), MonsterDyp::ID);
    assert!(matches!(
        field(&view, "format.rounds").input,
        Input::Integer { value: Some(7), .. }
    ));
    form.enter(&path("format.rounds"), "3").unwrap();
    assert!(session.edit_phase(form.draft()).unwrap());
    assert!(matches!(
        field(
            &session.phase_form_to_edit().unwrap().view(),
            "format.rounds"
        )
        .input,
        Input::Integer { value: Some(3), .. }
    ));
}

/// The fields of the phase and those of a plugin are said in the language, and else in English.
#[test]
fn the_label_of_a_field_is_said_in_the_language() {
    let mut form = new_form();
    form.choose(&path("format"), Some(MonsterDyp::ID)).unwrap();
    let view = form.view();
    let german = Translator::new(Language::German, &[]);

    assert_eq!(
        german.field_label(field(&view, "ranking")),
        "Reihenfolge der Tabelle"
    );
    assert_eq!(
        german.field_description(field(&view, "metrics")).as_deref(),
        Some("für jedes Team berechnet")
    );
    assert_eq!(german.field_label(field(&view, "format.rounds")), "Runden");
    assert_eq!(
        Translator::new(Language::English, &[]).field_label(field(&view, "metrics")),
        "metrics"
    );
}

#[test]
fn a_range_is_said_in_the_language() {
    let german = Translator::new(Language::German, &[]);
    let say = |range| {
        german
            .text(&with_range(Text::new("number-range"), range))
            .replace(['\u{2068}', '\u{2069}'], "")
    };

    assert_eq!(
        say(NumberRange::Integer(
            IntegerRange::new(1, 10).multiples_of(2)
        )),
        "eine ganze Zahl von 2 bis 10, die ein Vielfaches von 2 ist"
    );
    assert_eq!(
        say(NumberRange::Decimal(DecimalRange::ALL.multiples_of(0.5))),
        "eine Zahl, die ein Vielfaches von 0.5 ist"
    );
}
