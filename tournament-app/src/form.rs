//! The plugins of a phase being configured on a screen, as a form generated from their schema.
//!
//! A [`PhaseForm`] holds a [`Draft`] of a phase and hands out a [`FormView`] of it: every field of
//! the phase and of the plugins chosen, by kind and with its answer, the plugins each field can
//! take, the plugins the answers imply, and the issues of the answers at the fields where they are
//! fixed. A screen lays it out and hands what the operator does back to the form, by the
//! [`Path`] of the field it was done to; once done, it hands [`PhaseForm::draft`] to
//! [`Session::add_phase`](crate::session::Session::add_phase) or
//! [`Session::edit_phase`](crate::session::Session::edit_phase).
//!
//! The fields offer what the plugins chosen recommend, see [`Draft::default_value`], and a field
//! still answered as offered follows when a change of a field before it in schema order makes it
//! offer something else: choosing another format of a phase changes its ranking and metrics too,
//! unless the operator changed them, while removing a metric leaves the ranking as it is.

pub use plugin_factory::config::Path;
use plugin_factory::{
    catalog::{AnyAxis, PluginEntry},
    config::{Config, ConfigError, PluginConfig, Segment, Value},
    draft::{Draft, Issue, Problem},
    schema::{Configurable, DecimalRange, FieldKind, IntegerRange, NumberRange, Schema},
};
use tournament_engine::PhasePlugins;

use crate::i18n::{Arg, Text};

/// The plugins of a phase being configured, see the [module](self).
#[derive(Clone)]
pub struct PhaseForm {
    draft: Draft<PhasePlugins>,
    /// What was entered into a number field and not taken, with why, by the path of the field.
    /// Dropped by any change other than entering a number, which may move the fields.
    rejected: Vec<Rejected>,
}

#[derive(Debug, Clone)]
struct Rejected {
    path: Path,
    entered: String,
    problem: Text,
}

/// What a [`PhaseForm`] shows: plain data for a screen to lay out.
#[derive(Debug, Clone, PartialEq)]
pub struct FormView {
    /// The fields of the phase, in schema order.
    pub fields: Vec<FormField>,
    /// What is wrong with the answers that no field in particular fixes.
    pub issues: Vec<Text>,
    /// Whether nothing is wrong with the answers, so that a phase can be played with them.
    pub complete: bool,
}

/// A field of the phase or of a plugin chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct FormField {
    /// Where the field is, which the changes of [`PhaseForm`] take.
    pub path: Path,
    /// The id of the plugin whose field it is, or `None` for a field of the phase itself. Its
    /// texts are translated by it, see
    /// [`Translator::field_label`](crate::i18n::Translator::field_label).
    pub plugin: Option<&'static str>,
    pub name: &'static str,
    /// The label in English.
    pub label: &'static str,
    /// The description in English.
    pub description: Option<&'static str>,
    pub input: Input,
    /// What is wrong with the answer, to be fixed here.
    pub issues: Vec<Text>,
}

/// How a field is answered, with its answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// Yes or no, `None` while unanswered.
    Bool(Option<bool>),
    /// A whole number of `range`.
    Integer {
        range: IntegerRange,
        value: Option<i64>,
        /// What was entered last and not taken, which the field shows instead of `value`.
        entered: Option<String>,
        /// The numbers it takes, e.g. "a whole number of at least 1".
        hint: Text,
    },
    /// A number of `range`.
    Decimal {
        range: DecimalRange,
        value: Option<f64>,
        /// What was entered last and not taken, which the field shows instead of `value`.
        entered: Option<String>,
        /// The numbers it takes, e.g. "a number of at least 0".
        hint: Text,
    },
    /// An answer of `inner` that may be left out, see [`PhaseForm::answer_optional`].
    Optional { answered: bool, inner: Box<Input> },
    /// One plugin.
    Plugin(PluginInput),
    /// A list of plugins.
    List(ListInput),
}

/// A field taking one plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInput {
    /// Whether it may take no plugin.
    pub optional: bool,
    /// The plugin chosen, with its fields; `None` while there is none.
    pub chosen: Option<Chosen>,
    /// The plugins it can take, the chosen one included: the plugins recommended first, in the
    /// order they are recommended in, then the others by label.
    pub options: Vec<PluginOption>,
}

/// A plugin chosen, with its fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Chosen {
    pub plugin: PluginOption,
    pub fields: Vec<FormField>,
}

/// A field taking a list of plugins.
#[derive(Debug, Clone, PartialEq)]
pub struct ListInput {
    /// Whether the order of the plugins matters, e.g. that of the criteria the phase is ranked by;
    /// that of a set of metrics does not.
    pub ordered: bool,
    /// The plugins chosen, in order.
    pub items: Vec<Item>,
    /// The plugins added after them since the others need them, see [`Draft::add_implied`]. They
    /// have nothing to configure, and go when nothing needs them any more.
    pub implied: Vec<PluginOption>,
    /// The plugins that can be added, ordered as [`PluginInput::options`].
    pub options: Vec<PluginOption>,
}

/// A plugin chosen in a list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Where it is, which [`PhaseForm::remove`] and the moves take.
    pub path: Path,
    pub plugin: PluginOption,
    pub fields: Vec<FormField>,
    /// What is wrong with it, to be fixed by removing or moving it.
    pub issues: Vec<Text>,
}

/// A plugin as a choice of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PluginOption {
    /// Its id, which the changes of [`PhaseForm`] take, and which its texts are translated by, see
    /// [`PluginText`](crate::i18n::PluginText).
    pub id: &'static str,
    /// The label in English.
    pub label: &'static str,
    /// The description in English.
    pub description: Option<&'static str>,
    /// Whether the plugins chosen elsewhere recommend it here.
    pub recommended: bool,
}

impl PluginOption {
    fn of(entry: &'static PluginEntry, recommended: bool) -> Self {
        Self {
            id: entry.id(),
            label: entry.label(),
            description: entry.description(),
            recommended,
        }
    }
}

impl PhaseForm {
    /// A form of the phase `draft` configures, e.g.
    /// [`Session::phase_form`](crate::session::Session::phase_form).
    pub fn new(draft: Draft<PhasePlugins>) -> Self {
        Self {
            draft,
            rejected: Vec::new(),
        }
    }

    /// The draft as the form answered it.
    pub fn draft(&self) -> &Draft<PhasePlugins> {
        &self.draft
    }

    /// What the form shows now.
    pub fn view(&self) -> FormView {
        let mut completed = self.draft.clone();
        completed.add_implied();
        let issues = completed.build().err().unwrap_or_default();
        let mut fields = self.fields(
            &PhasePlugins::schema(),
            None,
            &Path::ROOT,
            completed.answers(),
        );
        let mut unplaced = Vec::new();
        for issue in &issues {
            let text = issue_text(issue, completed.answers());
            if !place(&mut fields, issue.path(), &text) {
                unplaced.push(text);
            }
        }
        for rejected in &self.rejected {
            place(&mut fields, &rejected.path, &rejected.problem);
        }
        FormView {
            fields,
            complete: issues.is_empty(),
            issues: unplaced,
        }
    }

    /// Answers the field at `path`, which takes yes or no, with `answer`.
    ///
    /// # Errors
    /// Returns an error if there is no such field, or it takes something else.
    pub fn set_bool(&mut self, path: &Path, answer: bool) -> Result<(), ConfigError> {
        self.change(path, |draft| draft.set(path, answer))
    }

    /// Answers the field at `path`, which takes a number, with the number `entered`, as an operator
    /// typed it: with a point or a comma before its decimals. Something else, or a number the field
    /// does not take, leaves the answer as it was, and the field shows what was entered with why
    /// it was not taken until the next change.
    ///
    /// # Errors
    /// Returns an error if there is no such field, or it takes no number.
    pub fn enter(&mut self, path: &Path, entered: &str) -> Result<(), ConfigError> {
        let kind = self.kind(path)?;
        let text = entered.trim().replace(',', ".");
        let number = match kind.range() {
            Some(NumberRange::Integer(_)) => text.parse::<i64>().ok().map(Value::from),
            Some(NumberRange::Decimal(_)) => text
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .map(Value::from),
            None => {
                return Err(ConfigError::WrongKind {
                    path: path.clone(),
                    expected: "an answer that is no number",
                    found: "a number",
                });
            }
        };
        self.rejected.retain(|rejected| rejected.path != *path);
        let problem = match number.map(|number| self.change(path, |draft| draft.set(path, number)))
        {
            Some(Ok(())) => return Ok(()),
            Some(Err(ConfigError::OutOfRange { allowed, .. })) => {
                with_range(Text::new("issue-out-of-range"), allowed)
            }
            Some(Err(error)) => return Err(error),
            None => Text::new("issue-not-a-number").with("entered", entered.trim()),
        };
        self.rejected.push(Rejected {
            path: path.clone(),
            entered: entered.to_owned(),
            problem,
        });
        Ok(())
    }

    /// Answers the field at `path`, which may be left without an answer, if `answered`, or else
    /// leaves it without one. An answer it is given is what it offers, or else no, or the number of
    /// its range closest to 0.
    ///
    /// # Errors
    /// Returns an error if there is no such field, or it takes an answer that cannot be left out.
    pub fn answer_optional(&mut self, path: &Path, answered: bool) -> Result<(), ConfigError> {
        let FieldKind::Optional(inner) = self.kind(path)? else {
            return Err(ConfigError::WrongKind {
                path: path.clone(),
                expected: "an answer that cannot be left out",
                found: "none",
            });
        };
        if !answered {
            return self.change(path, |draft| draft.set(path, Value::Null));
        }
        let offered = self
            .draft
            .default_value(path)?
            .filter(|offered| !offered.is_null());
        let answer = match (offered, &*inner) {
            (Some(offered), _) => offered,
            (None, FieldKind::Bool) => Value::from(false),
            // 0 is a multiple of anything, and so are the bounds
            (None, FieldKind::Integer(range)) => Value::from(0.clamp(range.min(), range.max())),
            (None, FieldKind::Decimal(range)) => {
                let at_least = range.min().map_or(0.0, |min| min.max(0.0));
                Value::from(range.max().map_or(at_least, |max| at_least.min(max)))
            }
            (None, _) => Value::Null,
        };
        self.change(path, |draft| draft.set(path, answer))
    }

    /// Answers the field at `path`, which takes one plugin, with the plugin `plugin`, configured as
    /// it offers, or with none for `None`.
    ///
    /// # Errors
    /// Returns an error if there is no such field, it takes no plugin, or no plugin of this id;
    /// for `None`, if it has to take one.
    pub fn choose(&mut self, path: &Path, plugin: Option<&str>) -> Result<(), ConfigError> {
        let answer = plugin.map_or(Value::Null, |plugin| PluginConfig::new(plugin).into());
        self.change(path, |draft| draft.set(path, answer))
    }

    /// Adds the plugin `plugin` to the end of the list at `path`, configured as it offers.
    ///
    /// # Errors
    /// Returns an error if there is no such field, it takes no list, or no plugin of this id, or
    /// has it already.
    pub fn add(&mut self, path: &Path, plugin: &str) -> Result<(), ConfigError> {
        let end = self.items(path)?.len();
        self.change(path, |draft| {
            draft.set(&path.clone().index(end), PluginConfig::new(plugin))
        })
    }

    /// Removes the plugin at `item`, a position in a list, from it.
    ///
    /// # Errors
    /// Returns an error if there is no such position.
    pub fn remove(&mut self, item: &Path) -> Result<(), ConfigError> {
        let (list, _) = position(item)?;
        self.change(&list, |draft| draft.unset(item))
    }

    /// Moves the plugin at `item`, a position in a list, one up, before the plugin before it;
    /// the first stays where it is.
    ///
    /// # Errors
    /// Returns an error if there is no such position.
    pub fn move_up(&mut self, item: &Path) -> Result<(), ConfigError> {
        let (list, index) = position(item)?;
        self.swap(&list, index.checked_sub(1), index)
    }

    /// Moves the plugin at `item`, a position in a list, one down, after the plugin after it; the
    /// last stays where it is.
    ///
    /// # Errors
    /// Returns an error if there is no such position.
    pub fn move_down(&mut self, item: &Path) -> Result<(), ConfigError> {
        let (list, index) = position(item)?;
        self.swap(&list, Some(index), index + 1)
    }

    /// Swaps the plugins at `first` and `second` of the list at `list`, unless one of them is not
    /// in it.
    fn swap(
        &mut self,
        list: &Path,
        first: Option<usize>,
        second: usize,
    ) -> Result<(), ConfigError> {
        let mut items = self.items(list)?;
        let Some(first) = first.filter(|_| second < items.len()) else {
            return Ok(());
        };
        items.swap(first, second);
        self.change(list, |draft| draft.set(list, Value::Array(items)))
    }

    /// The answers in the list at `path`.
    fn items(&self, path: &Path) -> Result<Vec<Value>, ConfigError> {
        match self.draft.answers().get_at(path) {
            Some(Value::Array(items)) => Ok(items.clone()),
            None if self.kind(path)?.axis().is_some() => Ok(Vec::new()),
            _ => Err(ConfigError::WrongKind {
                path: path.clone(),
                expected: "a list of plugins",
                found: "something else",
            }),
        }
    }

    /// The kind of answer the field at `path` takes.
    fn kind(&self, path: &Path) -> Result<FieldKind, ConfigError> {
        kind_at(self.draft.answers(), path)
            .ok_or_else(|| ConfigError::UnknownField { path: path.clone() })
    }

    /// Makes `change` to the answers of the field at `field`. Every field after it in schema
    /// order still answered as it offers before is answered as it offers after, with the plugins
    /// answering it as they offer, and so is every field `change` leaves unanswered. Nothing is
    /// changed if `change` fails.
    fn change(
        &mut self,
        field: &Path,
        change: impl FnOnce(&mut Draft<PhasePlugins>) -> Result<(), ConfigError>,
    ) -> Result<(), ConfigError> {
        // only the fields after the one changed follow, as the questionnaire asks a field after
        // those whose answers it offers what they recommend: a field before it following, e.g. the
        // ranking dropping a criterion when the metric it needs is removed, would change what the
        // operator may no longer look at, where an issue shows what is missing instead
        let paths = field_paths(self.draft.answers());
        let after = paths
            .iter()
            .position(|path| path == field)
            .map_or(paths.len(), |position| position + 1);
        let following: Vec<_> = paths
            .into_iter()
            .skip(after)
            // a field within the one changed is changed with it
            .filter(|path| !path.segments().starts_with(field.segments()))
            .filter(|path| as_offered(&self.draft, path))
            .collect();
        let mut draft = self.draft.clone();
        change(&mut draft)?;
        let mut answered_anew: Vec<Path> = Vec::new();
        for path in following {
            // those within a field answered anew are answered as offered with it
            if answered_anew
                .iter()
                .any(|anew| path.segments().starts_with(anew.segments()))
                || as_offered(&draft, &path)
            {
                continue;
            }
            draft.unset(&path)?;
            if let Some(offered) = draft.default_value(&path)? {
                draft.set(&path, offered)?;
            }
            answered_anew.push(path);
        }
        self.draft = draft.with_defaults()?;
        self.rejected.clear();
        Ok(())
    }

    /// The fields of `schema`, the schema of the config at `at`, which is that of the plugin
    /// `plugin` unless it is the phase's own. `completed` are the answers with the plugins they
    /// imply.
    fn fields(
        &self,
        schema: &Schema,
        plugin: Option<&'static str>,
        at: &Path,
        completed: &Config,
    ) -> Vec<FormField> {
        schema
            .fields()
            .iter()
            .map(|field| {
                let path = at.clone().field(field.name());
                FormField {
                    input: self.input(field.kind(), &path, completed),
                    path,
                    plugin,
                    name: field.name(),
                    label: field.label(),
                    description: field.description(),
                    issues: Vec::new(),
                }
            })
            .collect()
    }

    /// How the field of `kind` at `path` is answered.
    fn input(&self, kind: &FieldKind, path: &Path, completed: &Config) -> Input {
        let answer = self.draft.answers().get_at(path);
        let entered = || {
            self.rejected
                .iter()
                .find(|rejected| rejected.path == *path)
                .map(|rejected| rejected.entered.clone())
        };
        match kind {
            FieldKind::Bool => Input::Bool(answer.and_then(Value::as_bool)),
            FieldKind::Integer(range) => Input::Integer {
                range: *range,
                value: answer.and_then(Value::as_i64),
                entered: entered(),
                hint: with_range(Text::new("number-range"), NumberRange::Integer(*range)),
            },
            FieldKind::Decimal(range) => Input::Decimal {
                range: *range,
                value: answer.and_then(Value::as_f64),
                entered: entered(),
                hint: with_range(Text::new("number-range"), NumberRange::Decimal(*range)),
            },
            FieldKind::Plugin(axis) => Input::Plugin(self.plugin(*axis, path, false, completed)),
            FieldKind::List { axis, ordered } => {
                Input::List(self.list(*axis, *ordered, path, completed))
            }
            FieldKind::Optional(inner) => match &**inner {
                FieldKind::Plugin(axis) => Input::Plugin(self.plugin(*axis, path, true, completed)),
                FieldKind::List { .. } => self.input(inner, path, completed),
                _ => Input::Optional {
                    answered: answer.is_some_and(|answer| !answer.is_null()),
                    inner: Box::new(self.input(inner, path, completed)),
                },
            },
        }
    }

    /// The field taking one plugin of `axis` at `path`.
    fn plugin(
        &self,
        axis: AnyAxis,
        path: &Path,
        optional: bool,
        completed: &Config,
    ) -> PluginInput {
        let chosen = self
            .draft
            .answers()
            .get_at(path)
            .and_then(PluginConfig::from_answer)
            .and_then(|plugin| axis.entry(plugin.plugin()));
        let recommended = self.recommended(path);
        let options = self.options(path, &recommended, |id| {
            chosen.is_some_and(|chosen| chosen.id() == id)
        });
        PluginInput {
            optional,
            chosen: chosen.map(|entry| Chosen {
                plugin: PluginOption::of(entry, recommended.contains(&entry.id())),
                fields: self.fields(&entry.schema(), Some(entry.id()), path, completed),
            }),
            options,
        }
    }

    /// The field taking a list of plugins of `axis` at `path`.
    fn list(&self, axis: AnyAxis, ordered: bool, path: &Path, completed: &Config) -> ListInput {
        let entries = |answers: Option<&Value>| -> Vec<&'static PluginEntry> {
            answers
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|answer| axis.entry(PluginConfig::from_answer(answer)?.plugin()))
                .collect()
        };
        let chosen = entries(self.draft.answers().get_at(path));
        // implied plugins are appended to what was chosen
        let mut implied = entries(completed.get_at(path));
        let implied = implied.split_off(chosen.len().min(implied.len()));
        let recommended = self.recommended(path);
        let options = self
            .options(&path.clone().index(chosen.len()), &recommended, |_| false)
            .into_iter()
            .filter(|option| !implied.iter().any(|entry| entry.id() == option.id))
            .collect();
        ListInput {
            ordered,
            items: chosen
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let path = path.clone().index(index);
                    Item {
                        plugin: PluginOption::of(entry, recommended.contains(&entry.id())),
                        fields: self.fields(&entry.schema(), Some(entry.id()), &path, completed),
                        path,
                        issues: Vec::new(),
                    }
                })
                .collect(),
            implied: implied
                .into_iter()
                .map(|entry| PluginOption::of(entry, recommended.contains(&entry.id())))
                .collect(),
            options,
        }
    }

    /// The ids of the plugins the field at `path` offers.
    fn recommended(&self, path: &Path) -> Vec<&'static str> {
        let offered = self.draft.default_value(path).ok().flatten();
        let answers = match &offered {
            Some(Value::Array(answers)) => answers.iter().collect(),
            Some(answer) => vec![answer],
            None => Vec::new(),
        };
        let axis = self
            .kind(&list_field(path))
            .ok()
            .and_then(|kind| kind.axis());
        answers
            .into_iter()
            .filter_map(|answer| axis?.entry(PluginConfig::from_answer(answer)?.plugin()))
            .map(PluginEntry::id)
            .collect()
    }

    /// The plugins that fit the position at `path`, and those `kept` keeps although they do not,
    /// the plugins of `recommended` first.
    fn options(
        &self,
        path: &Path,
        recommended: &[&str],
        kept: impl Fn(&str) -> bool,
    ) -> Vec<PluginOption> {
        let mut options: Vec<_> = self
            .draft
            .candidates(path)
            .unwrap_or_default()
            .into_iter()
            .filter(|candidate| candidate.conflicts().is_empty() || kept(candidate.entry().id()))
            .map(|candidate| {
                let entry = candidate.entry();
                PluginOption::of(entry, recommended.contains(&entry.id()))
            })
            .collect();
        // the catalog lists plugins by id, which tells an operator nothing
        options.sort_by_cached_key(|option| {
            (
                recommended
                    .iter()
                    .position(|id| *id == option.id)
                    .unwrap_or(usize::MAX),
                option.label.to_lowercase(),
            )
        });
        options
    }
}

/// Whether the field at `path` of `draft` is answered as it offers, and the plugins answering it
/// as they offer.
fn as_offered(draft: &Draft<PhasePlugins>, path: &Path) -> bool {
    let Some(answer) = draft.answers().get_at(path) else {
        return false;
    };
    let Ok(Some(offered)) = draft.default_value(path) else {
        return false;
    };
    let mut as_offered = draft.clone();
    as_offered
        .set(path, offered)
        .ok()
        .and_then(|()| as_offered.with_defaults().ok())
        .is_some_and(|as_offered| as_offered.answers().get_at(path) == Some(answer))
}

/// The paths of the fields answered or to be answered in `answers`: those of the phase, each
/// followed by those of the plugins answering it, in schema order.
fn field_paths(answers: &Config) -> Vec<Path> {
    fn walk(schema: &Schema, answers: &Config, at: &Path, paths: &mut Vec<Path>) {
        for field in schema.fields() {
            let path = at.clone().field(field.name());
            paths.push(path.clone());
            let Some(axis) = field.kind().axis() else {
                continue;
            };
            let plugins = match answers.get_at(&path) {
                Some(Value::Array(answers)) => (0..answers.len())
                    .map(|index| path.clone().index(index))
                    .collect(),
                Some(_) => vec![path],
                None => Vec::new(),
            };
            for plugin in plugins {
                if let Some(entry) = answers
                    .get_at(&plugin)
                    .and_then(PluginConfig::from_answer)
                    .and_then(|config| axis.entry(config.plugin()))
                {
                    walk(&entry.schema(), answers, &plugin, paths);
                }
            }
        }
    }

    let mut paths = Vec::new();
    walk(&PhasePlugins::schema(), answers, &Path::ROOT, &mut paths);
    paths
}

/// Puts `text` at the deepest field or item of `fields` that `path` is within; `false` if it is
/// within none of them.
fn place(fields: &mut [FormField], path: &Path, text: &Text) -> bool {
    let Some(field) = fields
        .iter_mut()
        .find(|field| path.segments().starts_with(field.path.segments()))
    else {
        return false;
    };
    let placed = match &mut field.input {
        Input::Plugin(PluginInput {
            chosen: Some(chosen),
            ..
        }) => place(&mut chosen.fields, path, text),
        Input::List(list) => list
            .items
            .iter_mut()
            .find(|item| path.segments().starts_with(item.path.segments()))
            .is_some_and(|item| {
                place(&mut item.fields, path, text) || {
                    item.issues.push(text.clone());
                    true
                }
            }),
        _ => false,
    };
    if !placed {
        field.issues.push(text.clone());
    }
    true
}

/// What `issue` of the answers `answers` says.
fn issue_text(issue: &Issue, answers: &Config) -> Text {
    match issue.problem() {
        Problem::Invalid(ConfigError::Unanswered { .. }) => Text::new("issue-unanswered"),
        Problem::Invalid(ConfigError::OutOfRange { allowed, .. }) => {
            with_range(Text::new("issue-out-of-range"), *allowed)
        }
        Problem::Invalid(ConfigError::Duplicate { .. }) => Text::new("issue-duplicate"),
        Problem::Invalid(ConfigError::Rejected { reason, .. }) => {
            Text::new("issue-rejected").with("reason", reason.as_str())
        }
        Problem::Invalid(error) => Text::new("issue-invalid").with("reason", error.to_string()),
        Problem::Incompatible(incompatibility) => {
            Text::new("issue-incompatible").with("reason", incompatibility.to_string())
        }
        Problem::Unprovided {
            fact,
            required_by,
            providers,
        } => {
            let plugin = entry_at(answers, required_by)
                .map_or_else(|| Arg::from(required_by.to_string()), plugin_arg);
            // the providers are of the axis of the field the issue is at
            let axis = kind_at(answers, issue.path()).and_then(|kind| kind.axis());
            let providers: Vec<_> = providers
                .iter()
                .filter_map(|id| axis?.entry(id))
                .map(|entry| (entry.id(), entry.label()))
                .collect();
            if providers.is_empty() {
                Text::new("issue-unprovided")
                    .with("plugin", plugin)
                    .with("fact", *fact)
            } else {
                Text::new("issue-unprovided-by")
                    .with("plugin", plugin)
                    .with("providers", Arg::Plugins(providers))
            }
        }
    }
}

/// `text` with the variables describing `range`: `whole` (`yes` or `no`), `bounds` (`both`,
/// `min`, `max` or `none`), `min`, `max`, and `multiple`, which is 0 for any number.
#[expect(
    clippy::cast_precision_loss,
    reason = "a bound an operator reads is far below 2^52"
)]
fn with_range(text: Text, range: NumberRange) -> Text {
    let (whole, min, max, multiple) = match range {
        NumberRange::Integer(range) => (
            "yes",
            range.stated_min().map(|min| min as f64),
            range.stated_max().map(|max| max as f64),
            Some(range.multiple_of() as f64).filter(|multiple| *multiple != 1.0),
        ),
        NumberRange::Decimal(range) => ("no", range.min(), range.max(), range.multiple_of()),
    };
    let bounds = match (min, max) {
        (Some(_), Some(_)) => "both",
        (Some(_), None) => "min",
        (None, Some(_)) => "max",
        (None, None) => "none",
    };
    text.with("whole", whole)
        .with("bounds", bounds)
        .with("min", min.unwrap_or_default())
        .with("max", max.unwrap_or_default())
        .with("multiple", multiple.unwrap_or_default())
}

/// The label of the plugin `entry`, as a variable of a [`Text`].
fn plugin_arg(entry: &'static PluginEntry) -> Arg {
    Arg::Plugin {
        id: entry.id(),
        english: entry.label(),
    }
}

/// The kind of answer the field at `path` in `answers` takes, if there is such a field.
fn kind_at(answers: &Config, path: &Path) -> Option<FieldKind> {
    let Some(Segment::Field(name)) = path.segments().last() else {
        return None;
    };
    let schema = schema_at(answers, &parent(path))?;
    Some(schema.field_named(name)?.kind().clone())
}

/// The schema of the config at `config` in `answers`: the phase's at the root, else that of the
/// plugin answering the position.
fn schema_at(answers: &Config, config: &Path) -> Option<Schema> {
    if config.segments().is_empty() {
        return Some(PhasePlugins::schema());
    }
    entry_at(answers, config).map(PluginEntry::schema)
}

/// The plugin answering the position at `path` in `answers`, if one does.
fn entry_at(answers: &Config, path: &Path) -> Option<&'static PluginEntry> {
    let plugin = |at: &Path, axis: AnyAxis| {
        axis.entry(PluginConfig::from_answer(answers.get_at(at)?)?.plugin())
    };
    let mut schema = PhasePlugins::schema();
    let mut axis = None;
    let mut at = Path::ROOT;
    for segment in path.segments() {
        match segment {
            Segment::Field(name) => {
                if let Some(axis) = axis {
                    schema = plugin(&at, axis)?.schema();
                }
                axis = schema.field_named(name)?.kind().axis();
                at = at.field(name.clone());
            }
            Segment::Index(index) => at = at.index(*index),
        }
    }
    plugin(&at, axis?)
}

/// The path of the config the field at `path` is in: `path` without its last field, and without
/// a position in a list after it.
fn parent(path: &Path) -> Path {
    let segments = path.segments();
    let end = segments
        .iter()
        .rposition(|segment| matches!(segment, Segment::Field(_)))
        .unwrap_or(0);
    of(&segments[..end])
}

/// The field `path` is, or the list `path` is a position of.
fn list_field(path: &Path) -> Path {
    match path.segments().split_last() {
        Some((Segment::Index(_), list)) => of(list),
        _ => path.clone(),
    }
}

/// The list `item` is a position of, and the position.
fn position(item: &Path) -> Result<(Path, usize), ConfigError> {
    match item.segments().split_last() {
        Some((Segment::Index(index), list)) => Ok((of(list), *index)),
        _ => Err(ConfigError::UnknownField { path: item.clone() }),
    }
}

/// The path of `segments`.
fn of(segments: &[Segment]) -> Path {
    segments
        .iter()
        .fold(Path::ROOT, |path, segment| match segment {
            Segment::Field(name) => path.field(name.clone()),
            Segment::Index(index) => path.index(*index),
        })
}

#[cfg(test)]
mod tests;
