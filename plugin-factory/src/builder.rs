//! A composite of plugins being configured, one answer at a time.
//!
//! A [`Builder`] holds the answers given so far to the schema of a composite and to the schemas
//! of the plugins answering its fields, as one tree in which a [`Path`] addresses a field or a
//! position in a list. A UI renders the fields from the schemas along the
//! [answers](Builder::answers), see [`Builder`], and from the builder's queries — the
//! [default](Builder::default) of a field, the [candidates](Builder::candidates) for a position
//! taking plugins and the [issues](Builder::issues) of the answers so far — and changes the tree
//! through [`Builder::set`] and [`Builder::unset`] alone, so that no answer is ever changed behind
//! its back. [`Builder::finish`] builds the composite.
//!
//! While a field taking plugins is unanswered, whatever the plugins of its axis could provide may
//! still arrive, so the candidates and issues are optimistic about it; [`Builder::finish`] checks
//! with nothing open, see [`compatibility::check`].

use std::{any::TypeId, fmt::Write as _, marker::PhantomData, mem};

use serde::{Deserialize, Serialize};

use crate::{
    Plugin,
    catalog::{AnyAxis, PluginEntry},
    compatibility::{self, Declaration, Declared, Incompatibility, OpenFacts},
    config::{self, Config, ConfigError, Path, PluginConfig, Segment, Value},
    schema::{Configurable, Field, FieldKind, Schema},
};

/// The answers to the schema of the composite `T` given so far, see the [module](self).
///
/// # Examples
/// A UI renders the fields by walking the schema of the composite along the answers, and the
/// schema of each plugin answering a field along the config of that plugin, building the path of
/// each field on the way:
///
/// ```
/// use plugin_factory::{
///     Plugin, axis,
///     builder::Builder,
///     compatibility::Declaration,
///     config::{Config, Path, PluginConfig, Value},
///     register,
///     schema::{Configurable, FieldKind, Schema},
/// };
/// use serde::{Deserialize, Serialize};
///
/// #[axis]
/// pub trait Seeding: Plugin {}
///
/// #[derive(Debug, Serialize, Deserialize, Plugin)]
/// #[plugin(
///     id = "my_crate::random",
///     label = "random",
///     description = "entrants are shuffled"
/// )]
/// struct Random {
///     #[serde(default)]
///     #[plugin(label = "Keep the top seeds")]
///     keep_top: u32,
/// }
///
/// #[register]
/// impl Seeding for Random {}
///
/// #[derive(Deserialize, Configurable)]
/// struct Phase {
///     #[plugin(label = "Seedings")]
///     seedings: Vec<Box<dyn Seeding>>,
/// }
///
/// /// Every field of `schema`, the schema of the config at `at`, by path and label, each followed
/// /// by the fields of the plugins answering it in `config`.
/// fn fields(schema: &Schema, config: &Config, at: &Path, lines: &mut Vec<String>) {
///     for field in schema.fields() {
///         let path = at.clone().field(field.name());
///         lines.push(format!("{path}: {}", field.label()));
///         if let Some(answer) = config.get(field.name()) {
///             plugin_fields(field.kind(), answer, &path, lines);
///         }
///     }
/// }
///
/// /// The fields of the plugins in `answer`, an answer of `kind` at `at`.
/// fn plugin_fields(kind: &FieldKind, answer: &Value, at: &Path, lines: &mut Vec<String>) {
///     match (kind, answer) {
///         (FieldKind::Optional(inner), _) if !answer.is_null() => {
///             plugin_fields(inner, answer, at, lines);
///         }
///         (FieldKind::List(element), Value::Array(answers)) => {
///             for (index, answer) in answers.iter().enumerate() {
///                 let path = at.clone().index(index);
///                 plugin_fields(element, answer, &path, lines);
///             }
///         }
///         (FieldKind::Plugin(axis), _) => {
///             // an answer is checked when it is set, so it names a plugin of the axis
///             let plugin = PluginConfig::deserialize(answer).unwrap();
///             let schema = axis.entry(plugin.plugin()).unwrap().schema();
///             fields(&schema, plugin.config(), at, lines);
///         }
///         _ => {}
///     }
/// }
///
/// # fn main() {
/// let mut builder = Builder::<Phase>::new(Declaration::new());
/// builder
///     .set(
///         &"seedings".parse().unwrap(),
///         vec![PluginConfig::of::<Random>()],
///     )
///     .unwrap();
///
/// let mut lines = Vec::new();
/// fields(&Phase::schema(), builder.answers(), &Path::ROOT, &mut lines);
/// assert_eq!(
///     lines,
///     [
///         "seedings: Seedings",
///         "seedings[0].keep_top: Keep the top seeds"
///     ]
/// );
/// # }
/// ```
pub struct Builder<T> {
    context: Declaration,
    answers: Config,
    composite: PhantomData<fn() -> T>,
}

impl<T: Configurable> Builder<T> {
    /// A builder with nothing answered, for a composite with `context`, i.e. what is true of it
    /// before any plugin is chosen, see [`compatibility::check`].
    pub fn new(context: Declaration) -> Self {
        Self {
            context,
            answers: Config::new(),
            composite: PhantomData,
        }
    }

    /// A builder with every field answered as in `composite`, e.g. one that was loaded, for a
    /// composite with `context`; the answers are what `composite` serializes to. So
    /// `from_built(..).finish()` settles and checks a composite built elsewhere.
    ///
    /// # Errors
    /// Returns [`ConfigError::Rejected`] at the root if `composite` does not serialize to a
    /// config, and what [`Self::set`] returns for an answer that is not one of the schema.
    pub fn from_built(context: Declaration, composite: &T) -> Result<Self, ConfigError>
    where
        T: Serialize,
    {
        let answers = serde_json::to_value(composite)
            .and_then(Config::deserialize)
            .map_err(|error| ConfigError::Rejected {
                path: Path::ROOT,
                reason: error.to_string(),
            })?;
        Ok(Self {
            context,
            answers: T::schema().walk(&answers, &Path::ROOT, false)?,
            composite: PhantomData,
        })
    }

    /// The answers given so far, as the config of the composite.
    pub fn answers(&self) -> &Config {
        &self.answers
    }

    /// The answer the field at `path` offers, or [`None`] if it offers none or `path` addresses a
    /// position in a list rather than a field. It is never filled in: a UI offering it sets it
    /// as the answer, see [`Self::set`].
    ///
    /// A field taking plugins offers what the plugins answering the other fields of its config
    /// recommend, see [`Plugin::recommends`], as far as it is of its
    /// axis and fits as a [candidate](Self::candidates) would: a field taking one plugin the first
    /// such recommendation, a field taking a list all of them that fit together, in the order they
    /// are recommended in. A field no recommendation fits offers the default of its schema.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers.
    pub fn default(&self, path: &Path) -> Result<Option<Value>, ConfigError> {
        let schema = T::schema();
        let Some(field) = locate(&schema, &self.answers, path)?.field else {
            return Ok(None);
        };
        Ok(self.default_of(&Tree::of(&schema, &self.answers), &field, path))
    }

    /// The plugins that could answer the position at `path`, by id, each with why it does not fit
    /// if it does not; empty if the position takes no plugins.
    ///
    /// A candidate takes the place of what answers the position, so it is checked against the
    /// answers without it, and only for what it declares itself: an incompatibility among the
    /// other answers is not its conflict. What the fields still unanswered could provide is
    /// open, and so is what the candidate's own plugin fields could.
    /// For a field taking a list the candidate is one of the plugins answering it, so the rest of
    /// the list is open as well; at a position in the list, the rest of the list is as answered.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers. A path
    /// may address the position just past the end of a list.
    pub fn candidates(&self, path: &Path) -> Result<Vec<Candidate>, ConfigError> {
        let schema = T::schema();
        let position = locate(&schema, &self.answers, path)?;
        let Some((axis, list)) = slot(&position.kind) else {
            return Ok(Vec::new());
        };
        let tree = Tree::of(&schema, &self.answers);
        let declarations = tree.declarations(Some(path));
        let mut open = tree.open(Some(path));
        if list {
            open.extend(axis.facts().iter());
        }
        Ok(axis
            .entries()
            .map(|entry| {
                let mut open = open.clone();
                open.extend(entry.sub_axes().iter().flat_map(|axis| axis.facts().iter()));
                let conflict = compatibility::check_addition(
                    &self.context,
                    declarations.iter().copied(),
                    &Declared::new(entry.label(), entry.declaration()),
                    &open,
                )
                .err();
                Candidate { entry, conflict }
            })
            .collect())
    }

    /// Everything currently wrong with the answers, each at the path where it is fixed, in no
    /// particular order: an answer a plugin rejects, a plugin that does not fit next to the
    /// others, and a fact a plugin requires that nothing provides and no unanswered field could.
    ///
    /// An unanswered field is no issue, but still to be answered. A plugin with an unanswered
    /// field counts with its static declaration only and recommends nothing.
    pub fn issues(&self) -> Vec<Issue> {
        self.issues_in(&Tree::of(&T::schema(), &self.answers), false)
    }

    /// [`Self::issues`] of the answers `tree` holds. If `finished`, nothing is open any more: an
    /// unanswered field is an issue and provides nothing.
    fn issues_in(&self, tree: &Tree, finished: bool) -> Vec<Issue> {
        let mut issues = Vec::new();
        if finished {
            for question in tree
                .questions
                .iter()
                .filter(|question| question.answer.is_none())
            {
                issues.push(Issue {
                    path: question.path.clone(),
                    problem: Problem::Invalid(ConfigError::Unanswered {
                        path: question.path.clone(),
                    }),
                });
            }
        }
        for placed in &tree.plugins {
            if let Err(error) = &placed.instance
                && !matches!(error, ConfigError::Unanswered { .. })
            {
                let issue = Issue {
                    path: error.path().clone(),
                    problem: Problem::Invalid(error.clone()),
                };
                // a plugin is built with the plugins answering its fields, so an error of one of
                // those is found again building the plugin holding it
                if !issues.contains(&issue) {
                    issues.push(issue);
                }
            }
        }

        let open = if finished {
            OpenFacts::NOTHING
        } else {
            tree.open(None)
        };
        let mut unprovided = Vec::new();
        for (index, placed) in tree.plugins.iter().enumerate() {
            let others = tree
                .plugins
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != index)
                .map(|(_, other)| &other.declared);
            let mut incompatible = None;
            for conflict in compatibility::conflicts(&self.context, others, &placed.declared, &open)
            {
                match conflict.incompatibility {
                    // reported below, at the fields that could provide it
                    Incompatibility::FactNotProvided { fact, .. } => {
                        unprovided.push((conflict.fact, fact, placed.path.clone()));
                    }
                    incompatibility => {
                        incompatible.get_or_insert(incompatibility);
                    }
                }
            }
            if let Some(incompatibility) = incompatible {
                issues.push(Issue {
                    path: placed.path.clone(),
                    problem: Problem::Incompatible(incompatibility),
                });
            }
        }

        for (fact, name, required_by) in unprovided {
            // unless finished, a field still unanswered that could provide the fact leaves it
            // open, so only answered fields are found here
            let fields: Vec<_> = tree
                .questions
                .iter()
                .filter_map(|question| Some((question, slot(question.field.kind())?.0)))
                .filter(|(_, axis)| axis.facts().contains(fact))
                .collect();
            if fields.is_empty() {
                issues.push(Issue {
                    path: required_by.clone(),
                    problem: Problem::Unprovided {
                        fact: name,
                        required_by,
                        providers: Vec::new(),
                    },
                });
                continue;
            }
            for (question, axis) in fields {
                let providers: Vec<_> = axis
                    .entries()
                    .filter(|entry| entry.declaration().provides_type(fact))
                    .map(PluginEntry::id)
                    .collect();
                issues.push(Issue {
                    path: question.path.clone(),
                    problem: Problem::Unprovided {
                        fact: name,
                        required_by: required_by.clone(),
                        providers,
                    },
                });
            }
        }
        issues
    }

    /// Answers the position at `path` with `answer`: a field, a position in a list, or the
    /// position just past its end, which appends `answer` to the list.
    ///
    /// The answer may leave fields of the plugins in it unanswered. What a plugin rejects is not
    /// checked here but reported as an [issue](Self::issues).
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers or the
    /// answer answers a field a schema does not have, [`ConfigError::WrongKind`] if an answer is
    /// not one its field takes, and [`ConfigError::UnknownPlugin`] if a plugin in it is not
    /// registered for the axis of its field, with paths seen from the root. Nothing is changed
    /// then.
    pub fn set(&mut self, path: &Path, answer: impl Into<Value>) -> Result<(), ConfigError> {
        set_in(&T::schema(), &mut self.answers, path, &answer.into())
    }

    /// Takes back the answer at `path`: a field is unanswered again, and a position in a list is
    /// removed from it, so that the positions after it move up by one.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers. Nothing
    /// is changed then.
    pub fn unset(&mut self, path: &Path) -> Result<(), ConfigError> {
        let position = locate(&T::schema(), &self.answers, path)?;
        if position.field.is_none() && position.answer.is_none() {
            return Err(ConfigError::UnknownField { path: path.clone() });
        }
        edit(&mut self.answers, path, None);
        Ok(())
    }

    /// The composite, built from the answers with every fact settled that the catalog settles.
    ///
    /// Every field has to be answered; defaults are not filled in. A fact a plugin requires and
    /// neither the plugins nor the context provide is settled if exactly one plugin without
    /// fields provides it among the axes of the fields of the composite taking a list of plugins,
    /// and its axis is taken by one such field only: the plugin is appended to that field. A
    /// field taking one plugin, and a field of a plugin, is never added to. This repeats until
    /// nothing changes, so what a plugin added requires is settled in turn.
    ///
    /// Leaves the builder as it is, so that a failed composite can be fixed and finished again.
    ///
    /// # Errors
    /// Returns the [issues](Self::issues) of the answers so settled, with nothing open any more:
    /// an unanswered field is a [`Problem::Invalid`] issue with [`ConfigError::Unanswered`], and
    /// a fact nothing provides is [`Problem::Unprovided`].
    pub fn finish(&self) -> Result<T, Vec<Issue>> {
        let schema = T::schema();
        let invalid = |error: ConfigError| {
            vec![Issue {
                path: error.path().clone(),
                problem: Problem::Invalid(error),
            }]
        };
        let mut answers = self.answers.clone();
        let mut tried = Vec::new();
        while self.settle(&schema, &mut answers, &mut tried) {}
        let issues = self.issues_in(&Tree::of(&schema, &answers), true);
        if !issues.is_empty() {
            return Err(issues);
        }
        let walked = schema.walk(&answers, &Path::ROOT, true).map_err(invalid)?;
        config::deserialize(&config::complete::<T>(walked), &Path::ROOT).map_err(invalid)
    }

    /// Settles the first fact the plugins in `answers`, the answers to `schema`, require that
    /// neither they nor the context provide and that is not in `tried`, see [`Self::finish`].
    /// Each fact looked at is added to `tried`, so that it is tried once.
    ///
    /// Returns whether a plugin was added.
    fn settle(&self, schema: &Schema, answers: &mut Config, tried: &mut Vec<TypeId>) -> bool {
        let lists: Vec<_> = schema
            .fields()
            .iter()
            .filter_map(|field| match slot(field.kind()) {
                Some((axis, true)) => Some((field.name(), axis)),
                _ => None,
            })
            .collect();
        let tree = Tree::of(schema, answers);
        let declarations = tree.declarations(None);
        for fact in compatibility::unprovided(&declarations) {
            if self.context.provides_type(fact) || tried.contains(&fact) {
                continue;
            }
            tried.push(fact);
            // an axis taken by two fields offers each provider twice, so it is never settled
            let mut providers = lists.iter().flat_map(|(name, axis)| {
                // only a plugin without fields is complete without being asked anything
                axis.entries()
                    .filter(move |entry| {
                        entry.declaration().provides_type(fact)
                            && entry.instance(&Config::new(), &Path::ROOT).is_ok()
                    })
                    .map(move |entry| (*name, entry))
            });
            if let Some((name, entry)) = providers.next()
                && providers.next().is_none()
            {
                let mut list = match answers.get(name) {
                    Some(Value::Array(list)) => list.clone(),
                    _ => Vec::new(),
                };
                list.push(PluginConfig::new(entry.id()).into());
                answers.set(name, list);
                return true;
            }
        }
        false
    }

    /// [`Self::default`] of `field`, the field at `path` in the answers `tree` holds.
    fn default_of(&self, tree: &Tree, field: &Field, path: &Path) -> Option<Value> {
        let Some((axis, list)) = slot(field.kind()) else {
            return field.default_value().cloned();
        };
        let (_, parent) = path
            .segments()
            .split_last()
            .expect("the path of a field is not the root");
        let recommended: Vec<_> = tree
            .plugins
            .iter()
            .filter(|placed| placed.parent.segments() == parent && !placed.path.starts_with(path))
            .filter_map(|placed| placed.instance.as_ref().ok())
            .flat_map(|instance| instance.recommends())
            .filter_map(|recommended| Some((axis.entry(recommended.plugin())?, recommended)))
            .collect();

        let declarations = tree.declarations(Some(path));
        let mut open = tree.open(Some(path));
        if list {
            open.extend(axis.facts().iter());
        }
        let mut added = Vec::new();
        let mut fitting = Vec::new();
        for (entry, recommended) in recommended {
            // a recommendation is a whole answer, so what it declares includes the plugins
            // answering its fields
            let declaration = entry
                .declaration_for(recommended.config())
                .unwrap_or_else(|_| entry.declaration());
            let declared = Declared::new(entry.label(), declaration);
            if compatibility::check_addition(
                &self.context,
                declarations.iter().copied().chain(&added),
                &declared,
                &open,
            )
            .is_ok()
            {
                added.push(declared);
                fitting.push(Value::from(recommended));
                if !list {
                    break;
                }
            }
        }
        match (list, fitting.is_empty()) {
            (_, true) => field.default_value().cloned(),
            (true, false) => Some(Value::Array(fitting)),
            (false, false) => fitting.pop(),
        }
    }
}

/// A plugin that could answer a position taking plugins, see [`Builder::candidates`].
#[derive(Debug)]
pub struct Candidate {
    entry: &'static PluginEntry,
    conflict: Option<Incompatibility>,
}

impl Candidate {
    /// The plugin.
    pub fn entry(&self) -> &'static PluginEntry {
        self.entry
    }

    /// Why the plugin does not fit, or [`None`] if it does.
    pub fn conflict(&self) -> Option<&Incompatibility> {
        self.conflict.as_ref()
    }
}

/// Something currently wrong with the answers of a [`Builder`], at the path where it is fixed,
/// see [`Builder::issues`].
#[derive(Debug, PartialEq, Eq)]
pub struct Issue {
    path: Path,
    problem: Problem,
}

impl Issue {
    /// The field or position in a list where the issue is fixed.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What is wrong.
    pub fn problem(&self) -> &Problem {
        &self.problem
    }
}

/// What is wrong, see [`Issue`].
#[derive(Debug, PartialEq, Eq)]
pub enum Problem {
    /// The plugin an answer configures rejects it. The issue is at the path of the error.
    Invalid(ConfigError),
    /// The plugin at the issue's path does not fit next to the others.
    Incompatible(Incompatibility),
    /// The plugin at `required_by` requires `fact`, which nothing provides and no unanswered field
    /// could. The issue is at each field whose axis could provide it, with the ids of the plugins
    /// of that axis that do, or at `required_by` if there is none.
    Unprovided {
        fact: &'static str,
        required_by: Path,
        providers: Vec<&'static str>,
    },
}

/// The axis of the plugins a position of `kind` takes and whether it takes a list of them, or
/// [`None`] if it takes no plugin.
fn slot(kind: &FieldKind) -> Option<(AnyAxis, bool)> {
    match unwrapped(kind) {
        FieldKind::Plugin(axis) => Some((*axis, false)),
        FieldKind::List(element) => match unwrapped(element) {
            FieldKind::Plugin(axis) => Some((*axis, true)),
            _ => None,
        },
        _ => None,
    }
}

/// `kind` without the [`FieldKind::Optional`]s around it.
fn unwrapped(kind: &FieldKind) -> &FieldKind {
    match kind {
        FieldKind::Optional(inner) => unwrapped(inner),
        _ => kind,
    }
}

/// What is at a path of the answers, see [`locate`].
struct Position {
    /// The field, unless the path addresses a position in a list.
    field: Option<Field>,
    kind: FieldKind,
    answer: Option<Value>,
}

/// What is at `path` in `answers`, the answers to `schema`.
///
/// # Errors
/// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers: the root, a
/// field its config does not have, a field of a plugin that is not chosen, or a position in a
/// list further than just past its end.
fn locate(schema: &Schema, answers: &Config, path: &Path) -> Result<Position, ConfigError> {
    let unknown = || ConfigError::UnknownField { path: path.clone() };
    let mut segments = path.segments().iter();
    let Some(Segment::Field(name)) = segments.next() else {
        return Err(unknown());
    };
    let field = schema.field_named(name).ok_or_else(unknown)?;
    let mut position = Position {
        field: Some(field.clone()),
        kind: field.kind().clone(),
        answer: answers.get(name).cloned(),
    };
    for segment in segments {
        position = match segment {
            Segment::Index(index) => {
                let (FieldKind::List(element), Some(Value::Array(answers))) =
                    (unwrapped(&position.kind), &position.answer)
                else {
                    return Err(unknown());
                };
                if *index > answers.len() {
                    return Err(unknown());
                }
                Position {
                    field: None,
                    kind: (**element).clone(),
                    answer: answers.get(*index).cloned(),
                }
            }
            Segment::Field(name) => {
                let FieldKind::Plugin(axis) = unwrapped(&position.kind) else {
                    return Err(unknown());
                };
                let plugin = position
                    .answer
                    .as_ref()
                    .and_then(PluginConfig::from_answer)
                    .ok_or_else(unknown)?;
                let schema = axis.entry(plugin.plugin()).ok_or_else(unknown)?.schema();
                let field = schema.field_named(name).ok_or_else(unknown)?;
                Position {
                    field: Some(field.clone()),
                    kind: field.kind().clone(),
                    answer: plugin.config().get(name).cloned(),
                }
            }
        };
    }
    Ok(position)
}

/// [`Builder::set`] on `answers`, the answers to `schema`.
fn set_in(
    schema: &Schema,
    answers: &mut Config,
    path: &Path,
    answer: &Value,
) -> Result<(), ConfigError> {
    let position = locate(schema, answers, path)?;
    let answer = position.kind.walk(answer, path, false)?;
    edit(answers, path, Some(answer));
    Ok(())
}

/// Answers the position at `path` in `answers` with `answer`, or takes its answer back if
/// `answer` is [`None`], see [`Builder::set`] and [`Builder::unset`]. `path` has been located.
fn edit(answers: &mut Config, path: &Path, answer: Option<Value>) {
    let (last, parent) = path
        .segments()
        .split_last()
        .expect("a located path is not the root");
    let mut root = Value::Object(mem::take(&mut answers.0));
    let holder = root
        .pointer_mut(&pointer(parent))
        .expect("a located path is in the answers");
    match last {
        Segment::Field(name) => {
            // the answers of a plugin are its answer's "value", which checking an answer writes
            // as an object
            let config = if parent.is_empty() {
                holder
            } else {
                &mut holder["value"]
            };
            let config = config
                .as_object_mut()
                .expect("the config of a plugin is an object");
            match answer {
                Some(answer) => config.insert(name.clone(), answer),
                None => config.remove(name),
            };
        }
        Segment::Index(index) => {
            let list = holder.as_array_mut().expect("a located index is in a list");
            match answer {
                Some(answer) if *index == list.len() => list.push(answer),
                Some(answer) => list[*index] = answer,
                None => {
                    list.remove(*index);
                }
            }
        }
    }
    let Value::Object(root) = root else {
        unreachable!("the root stays an object");
    };
    answers.0 = root;
}

/// The JSON pointer to the answer at `segments` in the answers as a [`Value`].
fn pointer(segments: &[Segment]) -> String {
    let mut pointer = String::new();
    for (position, segment) in segments.iter().enumerate() {
        match segment {
            Segment::Field(name) => {
                // a field after the first is one of the plugin answering what comes before it
                if position > 0 {
                    pointer.push_str("/value");
                }
                let name = name.replace('~', "~0").replace('/', "~1");
                write!(pointer, "/{name}").expect("writing to a string cannot fail");
            }
            Segment::Index(index) => {
                write!(pointer, "/{index}").expect("writing to a string cannot fail");
            }
        }
    }
    pointer
}

/// One field of the tree of answers.
struct Question {
    path: Path,
    field: Field,
    answer: Option<Value>,
}

/// The answers as a tree: every field and every plugin answering one.
struct Tree {
    /// Every field, depth first in schema order.
    questions: Vec<Question>,
    /// Every plugin answering a field or a position in a list, depth first in schema order.
    plugins: Vec<Placed>,
}

/// A plugin in the answers, built once with the tree.
struct Placed {
    path: Path,
    /// The path of the config whose field the plugin answers.
    parent: Path,
    /// The plugin built from its config, or why it cannot be, e.g. because a field is still
    /// unanswered, see [`PluginEntry::instance`].
    instance: Result<Box<dyn Plugin>, ConfigError>,
    /// What the plugin declares itself, without the plugins answering its fields, which are
    /// placed in the tree themselves: what its configuration declares if it is built, and its
    /// static declaration otherwise. Named by its label and path.
    declared: Declared,
}

impl Tree {
    /// The tree of `answers`, the answers to `schema`.
    fn of(schema: &Schema, answers: &Config) -> Self {
        let mut tree = Self {
            questions: Vec::new(),
            plugins: Vec::new(),
        };
        tree.add_config(schema, answers, &Path::ROOT);
        tree
    }

    fn add_config(&mut self, schema: &Schema, config: &Config, at: &Path) {
        for field in schema.fields() {
            let path = at.clone().field(field.name());
            let answer = config.get(field.name()).cloned();
            self.questions.push(Question {
                path: path.clone(),
                field: field.clone(),
                answer: answer.clone(),
            });
            if let Some(answer) = &answer {
                self.add_answer(field.kind(), answer, path, at);
            }
        }
    }

    /// Adds the plugins in `answer`, an answer of `kind` at `path` in the config at `parent`.
    ///
    /// The answers are checked when they are set, so an answer that is not of its kind is nothing
    /// but an answer of nothing to an optional field.
    fn add_answer(&mut self, kind: &FieldKind, answer: &Value, path: Path, parent: &Path) {
        match (kind, answer) {
            (FieldKind::Optional(inner), _) => self.add_answer(inner, answer, path, parent),
            (FieldKind::List(element), Value::Array(answers)) => {
                for (index, answer) in answers.iter().enumerate() {
                    self.add_answer(element, answer, path.clone().index(index), parent);
                }
            }
            (FieldKind::Plugin(axis), _) => {
                let Some(plugin) = PluginConfig::from_answer(answer) else {
                    return;
                };
                let Some(entry) = axis.entry(plugin.plugin()) else {
                    return;
                };
                self.plugins
                    .push(Placed::new(entry, plugin.config(), path.clone(), parent));
                self.add_config(&entry.schema(), plugin.config(), &path);
            }
            _ => {}
        }
    }

    /// The types of the facts the fields still unanswered could provide, except the fields at
    /// and within `excluding`.
    fn open(&self, excluding: Option<&Path>) -> OpenFacts {
        let mut axes = Vec::new();
        for question in &self.questions {
            if question.answer.is_none()
                && !is_within(&question.path, excluding)
                && let Some((axis, _)) = slot(question.field.kind())
                && !axes.contains(&axis)
            {
                axes.push(axis);
            }
        }
        axes.iter().flat_map(|axis| axis.facts().iter()).collect()
    }

    /// What the plugins declare, in the order of [`Self::plugins`], except the plugins at and
    /// within `excluding`.
    fn declarations(&self, excluding: Option<&Path>) -> Vec<&Declared> {
        self.plugins
            .iter()
            .filter(|placed| !is_within(&placed.path, excluding))
            .map(|placed| &placed.declared)
            .collect()
    }
}

/// Whether `path` is `excluding` or within it.
fn is_within(path: &Path, excluding: Option<&Path>) -> bool {
    excluding.is_some_and(|excluding| path.starts_with(excluding))
}

impl Placed {
    /// The plugin `entry` configured by `config`, answering the position at `path` of the config
    /// at `parent`.
    fn new(entry: &'static PluginEntry, config: &Config, path: Path, parent: &Path) -> Self {
        let instance = entry.instance(config, &path);
        let mut declaration = entry.declaration();
        if let Ok(instance) = &instance {
            instance.declare(&mut declaration);
        }
        let declared = Declared::new(format!("{} ({path})", entry.label()), declaration);
        Self {
            path,
            parent: parent.clone(),
            instance,
            declared,
        }
    }
}

/// The builder, tested on axes of its own so that it does not depend on what the plugins of a
/// program happen to register.
#[cfg(test)]
mod tests {
    use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
    use serde_json::json;

    use super::*;
    use crate::{Plugin, PluginType, axis, register};

    #[derive(Debug)]
    struct Points;

    #[derive(Debug)]
    struct Matches;

    #[derive(Debug)]
    struct Eliminating;

    /// How many entrants a ranking stands for, the fact the context of the composite provides.
    #[derive(Debug)]
    struct EntrantsPerRanking(u32);

    /// How the matches of a phase are scheduled.
    #[axis]
    trait Format: Plugin {}

    /// What is calculated for an entrant.
    #[axis]
    trait Metric: Plugin {}

    /// A format recommending metrics, two of which provide the same fact.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "round_robin",
        label = "round_robin",
        description = "a plugin of the builder's tests",
        recommends = Self::recommends
    )]
    struct RoundRobin;

    impl RoundRobin {
        fn recommends(&self) -> Vec<PluginConfig> {
            vec![
                PluginConfig::of::<MatchCount>(),
                PluginConfig::of::<Wins>(),
                PluginConfig::of::<Score>(),
            ]
        }
    }

    #[register]
    impl Format for RoundRobin {}

    /// A format with a parameter, providing a fact no metric provides.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "knockout",
        label = "knockout",
        description = "a plugin of the builder's tests",
        declare_static = Self::declare_static
    )]
    struct Knockout {
        #[serde(default)]
        #[plugin(label = "Third place")]
        third_place: bool,
    }

    impl Knockout {
        fn declare_static(declaration: &mut Declaration) {
            declaration.provide(Eliminating);
        }
    }

    #[register]
    impl Format for Knockout {}

    /// A format providing a fact the context provides.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "doubles",
        label = "doubles",
        description = "a plugin of the builder's tests",
        declare_static = Self::declare_static
    )]
    struct Doubles;

    impl Doubles {
        fn declare_static(declaration: &mut Declaration) {
            declaration.provide(EntrantsPerRanking(2));
        }
    }

    #[register]
    impl Format for Doubles {}

    /// A format with a condition on the fact the context provides.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "monster",
        label = "monster",
        description = "a plugin of the builder's tests",
        declare_static = Self::declare_static
    )]
    struct Monster;

    impl Monster {
        fn declare_static(declaration: &mut Declaration) {
            declaration.require_that::<EntrantsPerRanking>(
                "the entrants per ranking",
                "rankings of single players",
                |entrants| entrants.0 == 1,
            );
        }
    }

    #[register]
    impl Format for Monster {}

    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "match_count",
        label = "match_count",
        description = "a plugin of the builder's tests",
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

    /// A metric with a parameter it validates.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "wins",
        label = "wins",
        description = "a plugin of the builder's tests",
        declare_static = Self::declare_static
    )]
    struct Wins {
        #[serde(default = "one", deserialize_with = "non_negative")]
        #[plugin(label = "Points for a win")]
        for_win: f64,
    }

    impl Wins {
        fn declare_static(declaration: &mut Declaration) {
            declaration.provide(Points);
        }
    }

    fn one() -> f64 {
        1.0
    }

    fn non_negative<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let points = f64::deserialize(deserializer)?;
        if points < 0.0 {
            return Err(D::Error::custom("the points for a win cannot be negative"));
        }
        Ok(points)
    }

    #[register]
    impl Metric for Wins {}

    /// A metric providing what [`Wins`] provides.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "score",
        label = "score",
        description = "a plugin of the builder's tests",
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

    /// A metric requiring what other metrics provide.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "buchholz",
        label = "buchholz",
        description = "a plugin of the builder's tests",
        declare_static = Self::declare_static
    )]
    struct Buchholz;

    impl Buchholz {
        fn declare_static(declaration: &mut Declaration) {
            declaration
                .require::<Points>("points")
                .require::<Matches>("matches");
        }
    }

    #[register]
    impl Metric for Buchholz {}

    /// A metric requiring what only a format provides.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "losses",
        label = "losses",
        description = "a plugin of the builder's tests",
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
        id = "combined",
        label = "combined",
        description = "a metric made of metrics"
    )]
    struct Combined {
        #[serde(default)]
        #[plugin(label = "Parts")]
        parts: Vec<Box<dyn Metric>>,
    }

    #[register]
    impl Metric for Combined {}

    /// The composite: one format, any number of metrics, and a metric or none.
    #[derive(Debug, Serialize, Deserialize, Configurable)]
    struct Phase {
        #[plugin(label = "Format")]
        format: Box<dyn Format>,
        #[serde(default)]
        #[plugin(label = "Metrics")]
        metrics: Vec<Box<dyn Metric>>,
        #[plugin(label = "Tie break")]
        tie_break: Option<Box<dyn Metric>>,
    }

    /// A builder of a phase of single players.
    fn builder() -> Builder<Phase> {
        Builder::new(Declaration::of(|declaration| {
            declaration.provide(EntrantsPerRanking(1));
        }))
    }

    /// The path written as `text`, e.g. `metrics[1].for_win`.
    fn path(text: &str) -> Path {
        text.parse().unwrap()
    }

    /// The plugin `P` with nothing answered.
    fn plugin<P: PluginType>() -> PluginConfig {
        PluginConfig::of::<P>()
    }

    /// A builder with `answers` set, each at the path written as its first element.
    fn answered(answers: &[(&str, Value)]) -> Builder<Phase> {
        let mut builder = builder();
        for (at, answer) in answers {
            builder.set(&path(at), answer.clone()).unwrap();
        }
        builder
    }

    /// The candidates at `at`, by id, each with why it does not fit.
    fn candidates(
        builder: &Builder<Phase>,
        at: &str,
    ) -> Vec<(&'static str, Option<Incompatibility>)> {
        builder
            .candidates(&path(at))
            .unwrap()
            .into_iter()
            .map(|candidate| (candidate.entry().id(), candidate.conflict))
            .collect()
    }

    /// Why the plugin `id` does not fit at `at`.
    fn conflict_of(builder: &Builder<Phase>, at: &str, id: &str) -> Option<Incompatibility> {
        candidates(builder, at)
            .into_iter()
            .find(|(candidate, _)| *candidate == id)
            .expect("the plugin is a candidate")
            .1
    }

    /// The paths of the fields in the tree of the answers of `builder`, each with whether it is
    /// answered.
    fn paths<T: Configurable>(builder: &Builder<T>) -> Vec<(String, bool)> {
        Tree::of(&T::schema(), builder.answers())
            .questions
            .iter()
            .map(|question| (question.path.to_string(), question.answer.is_some()))
            .collect()
    }

    #[test]
    fn the_tree_holds_the_fields_of_the_composite_and_of_the_plugins_answering_them() {
        let mut builder = builder();

        assert_eq!(
            paths(&builder),
            [
                ("format".to_owned(), false),
                ("metrics".to_owned(), false),
                ("tie_break".to_owned(), false),
            ]
        );
        builder.set(&path("format"), plugin::<Knockout>()).unwrap();
        assert_eq!(
            paths(&builder),
            [
                ("format".to_owned(), true),
                ("format.third_place".to_owned(), false),
                ("metrics".to_owned(), false),
                ("tie_break".to_owned(), false),
            ]
        );
    }

    #[test]
    fn an_answer_is_set_at_its_path_and_unset_again() {
        let mut builder = answered(&[("metrics", json!([plugin::<Wins>()]))]);

        builder.set(&path("metrics[0].for_win"), 2).unwrap();
        builder
            .set(&path("metrics[1]"), plugin::<MatchCount>())
            .unwrap();
        assert_eq!(
            builder.answers().get("metrics"),
            Some(&json!([
                {"type": "wins", "value": {"for_win": 2}},
                {"type": "match_count", "value": {}},
            ]))
        );

        builder.unset(&path("metrics[0]")).unwrap();
        assert_eq!(
            builder.answers().get("metrics"),
            Some(&json!([{"type": "match_count", "value": {}}]))
        );
        builder.unset(&path("metrics")).unwrap();
        assert_eq!(builder.answers().get("metrics"), None);
    }

    #[test]
    fn a_path_addressing_nothing_is_an_error() {
        let mut builder = answered(&[("metrics", json!([]))]);

        for at in ["format.third_place", "metrics[1]", "rounds", "tie_break[0]"] {
            assert_eq!(
                builder.set(&path(at), 1),
                Err(ConfigError::UnknownField { path: path(at) }),
                "{at}"
            );
        }
        assert_eq!(
            builder.unset(&path("metrics[0]")),
            Err(ConfigError::UnknownField {
                path: path("metrics[0]"),
            })
        );
    }

    #[test]
    fn an_answer_that_is_not_one_its_field_takes_is_not_set() {
        let mut builder = builder();

        assert_eq!(
            builder.set(&path("metrics"), 1),
            Err(ConfigError::WrongKind {
                path: path("metrics"),
                expected: "a list",
                found: "a whole number",
            })
        );
        assert_eq!(
            builder.set(
                &path("metrics"),
                json!([plugin::<Wins>().with("for_win", true)])
            ),
            Err(ConfigError::WrongKind {
                path: path("metrics[0].for_win"),
                expected: "a number",
                found: "yes or no",
            })
        );
        assert_eq!(
            builder.set(&path("format"), plugin::<Wins>()),
            Err(ConfigError::UnknownPlugin {
                path: path("format"),
            })
        );
        assert_eq!(builder.answers(), &Config::new());
    }

    #[test]
    fn a_candidate_conflicting_with_the_context_says_why() {
        assert_eq!(
            candidates(&builder(), "format"),
            [
                (
                    "doubles",
                    Some(Incompatibility::FactProvidedByContext {
                        fact: format!("{:?}", EntrantsPerRanking(2)),
                        provider: "doubles".to_owned(),
                    })
                ),
                ("knockout", None),
                ("monster", None),
                ("round_robin", None),
            ]
        );
    }

    #[test]
    fn a_candidate_does_not_conflict_with_what_it_replaces() {
        let builder = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Wins>()])),
            ("tie_break", Value::Null),
        ]);

        assert_eq!(conflict_of(&builder, "metrics[0]", "score"), None);
        assert_eq!(conflict_of(&builder, "metrics", "score"), None);
        assert_eq!(
            conflict_of(&builder, "metrics[1]", "score"),
            Some(Incompatibility::FactProvidedTwice {
                fact: format!("{Points:?}"),
                first: "wins (metrics[0])".to_owned(),
                second: "score".to_owned(),
            })
        );
    }

    #[test]
    fn a_candidate_may_require_what_an_unanswered_field_could_provide() {
        let mut builder = answered(&[("tie_break", Value::Null)]);

        assert_eq!(conflict_of(&builder, "metrics", "losses"), None);
        builder
            .set(&path("format"), plugin::<RoundRobin>())
            .unwrap();
        assert_eq!(
            conflict_of(&builder, "metrics", "losses"),
            Some(Incompatibility::FactNotProvided {
                fact: "eliminations",
                required_by: "losses".to_owned(),
            })
        );
    }

    #[test]
    fn a_candidate_for_a_list_may_require_what_the_rest_of_the_list_could_provide() {
        let builder = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([])),
            ("tie_break", Value::Null),
        ]);

        assert_eq!(conflict_of(&builder, "metrics", "buchholz"), None);
        assert_eq!(
            conflict_of(&builder, "metrics[0]", "buchholz"),
            Some(Incompatibility::FactNotProvided {
                fact: "points",
                required_by: "buchholz".to_owned(),
            })
        );
    }

    #[test]
    fn a_position_taking_no_plugins_has_no_candidates() {
        let builder = answered(&[("format", plugin::<Knockout>().into())]);

        assert!(candidates(&builder, "format.third_place").is_empty());
    }

    #[test]
    fn nested_plugins_are_addressed_through_the_fields_they_answer() {
        let mut builder = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Combined>()])),
        ]);

        assert!(paths(&builder).contains(&("metrics[0].parts".to_owned(), false)));
        builder
            .set(&path("metrics[0].parts"), json!([plugin::<Wins>()]))
            .unwrap();
        assert_eq!(
            conflict_of(&builder, "metrics[0].parts[1]", "score"),
            Some(Incompatibility::FactProvidedTwice {
                fact: format!("{Points:?}"),
                first: "wins (metrics[0].parts[0])".to_owned(),
                second: "score".to_owned(),
            })
        );
    }

    #[test]
    fn an_answer_a_plugin_rejects_is_an_issue_at_its_field() {
        let builder = answered(&[("metrics", json!([plugin::<Wins>().with("for_win", -1.0)]))]);
        let rejected = ConfigError::Rejected {
            path: path("metrics[0].for_win"),
            reason: "the points for a win cannot be negative".to_owned(),
        };

        assert_eq!(
            builder.issues(),
            [Issue {
                path: path("metrics[0].for_win"),
                problem: Problem::Invalid(rejected),
            }]
        );
    }

    #[test]
    fn plugins_providing_the_same_fact_are_issues_at_each_of_them() {
        let builder = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Wins>(), plugin::<Score>()])),
            ("tie_break", Value::Null),
        ]);
        let twice = |first: &str, second: &str| {
            Problem::Incompatible(Incompatibility::FactProvidedTwice {
                fact: format!("{Points:?}"),
                first: first.to_owned(),
                second: second.to_owned(),
            })
        };

        assert_eq!(
            builder.issues(),
            [
                Issue {
                    path: path("metrics[0]"),
                    problem: twice("score (metrics[1])", "wins (metrics[0])"),
                },
                Issue {
                    path: path("metrics[1]"),
                    problem: twice("wins (metrics[0])", "score (metrics[1])"),
                },
            ]
        );
    }

    #[test]
    fn a_fact_nothing_provides_is_an_issue_at_each_field_that_could_provide_it() {
        let losing = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Losses>()])),
            ("tie_break", Value::Null),
        ]);
        let counting = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Buchholz>(), plugin::<Wins>()])),
            ("tie_break", Value::Null),
        ]);
        let unprovided = |fact, providers: &[&'static str]| Problem::Unprovided {
            fact,
            required_by: path("metrics[0]"),
            providers: providers.to_vec(),
        };

        assert_eq!(
            losing.issues(),
            [Issue {
                path: path("format"),
                problem: unprovided("eliminations", &["knockout"]),
            }]
        );
        assert_eq!(
            counting.issues(),
            [
                Issue {
                    path: path("metrics"),
                    problem: unprovided("matches", &["match_count"]),
                },
                Issue {
                    path: path("tie_break"),
                    problem: unprovided("matches", &["match_count"]),
                },
            ]
        );
    }

    #[test]
    fn a_fact_an_unanswered_field_could_provide_is_no_issue() {
        let builder = answered(&[("metrics", json!([plugin::<Losses>()]))]);

        assert_eq!(builder.issues(), []);
    }

    #[test]
    fn a_field_offers_the_default_of_its_schema() {
        let builder = answered(&[("format", plugin::<Knockout>().into())]);

        assert_eq!(
            builder.default(&path("format.third_place")),
            Ok(Some(json!(false)))
        );
        assert_eq!(builder.default(&path("metrics")), Ok(Some(json!([]))));
        assert_eq!(builder.default(&path("format")), Ok(None));
    }

    #[test]
    fn a_field_taking_a_list_offers_every_recommendation_that_fits() {
        let builder = answered(&[("format", plugin::<RoundRobin>().into())]);

        assert_eq!(
            builder.default(&path("metrics")),
            Ok(Some(json!([plugin::<MatchCount>(), plugin::<Wins>()])))
        );
    }

    #[test]
    fn a_field_taking_one_plugin_offers_the_first_recommendation_that_fits() {
        let mut builder = answered(&[("format", plugin::<RoundRobin>().into())]);

        assert_eq!(
            builder.default(&path("tie_break")),
            Ok(Some(plugin::<MatchCount>().into()))
        );
        builder
            .set(&path("metrics"), json!([plugin::<MatchCount>()]))
            .unwrap();
        assert_eq!(
            builder.default(&path("tie_break")),
            Ok(Some(plugin::<Wins>().into()))
        );
    }

    /// Answers every unanswered field of `builder` that offers an answer with it, depth first in
    /// schema order, as a UI accepting every default does.
    fn accept_defaults(builder: &mut Builder<Phase>) {
        while let Some(at) = Tree::of(&Phase::schema(), builder.answers())
            .questions
            .into_iter()
            .find(|question| {
                question.answer.is_none() && builder.default(&question.path) != Ok(None)
            })
            .map(|question| question.path)
        {
            let default = builder.default(&at).unwrap().unwrap();
            builder.set(&at, default).unwrap();
        }
    }

    #[test]
    fn finishing_fills_in_no_default() {
        let mut builder = answered(&[("format", plugin::<RoundRobin>().into())]);
        let unanswered = |at: &str| Issue {
            path: path(at),
            problem: Problem::Invalid(ConfigError::Unanswered { path: path(at) }),
        };

        assert_eq!(
            builder.finish().err(),
            Some(vec![unanswered("metrics"), unanswered("tie_break")])
        );
        accept_defaults(&mut builder);
        assert_eq!(
            builder.finish().map(|phase| format!("{phase:?}")),
            Ok(format!(
                "{:?}",
                Phase {
                    format: Box::new(RoundRobin),
                    metrics: vec![Box::new(MatchCount), Box::new(Wins { for_win: 1.0 })],
                    tie_break: None,
                }
            ))
        );
    }

    #[test]
    fn finishing_with_an_unanswered_field_fails() {
        let unanswered = |at: &str| Issue {
            path: path(at),
            problem: Problem::Invalid(ConfigError::Unanswered { path: path(at) }),
        };

        assert_eq!(
            builder().finish().err(),
            Some(vec![
                unanswered("format"),
                unanswered("metrics"),
                unanswered("tie_break"),
            ])
        );
    }

    #[test]
    fn a_composite_that_is_a_unit_struct_finishes() {
        /// A composite asking for nothing.
        #[derive(Debug, Deserialize, Configurable)]
        struct Nothing;

        assert!(Builder::<Nothing>::new(Declaration::new()).finish().is_ok());
    }

    #[test]
    fn finishing_with_an_answer_a_plugin_rejects_fails() {
        let builder = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Wins>().with("for_win", -1.0)])),
            ("tie_break", Value::Null),
        ]);

        assert_eq!(
            builder.finish().err(),
            Some(vec![Issue {
                path: path("metrics[0].for_win"),
                problem: Problem::Invalid(ConfigError::Rejected {
                    path: path("metrics[0].for_win"),
                    reason: "the points for a win cannot be negative".to_owned(),
                }),
            }])
        );
    }

    #[test]
    fn finishing_reports_with_nothing_open() {
        let builder = answered(&[
            ("metrics", json!([plugin::<Losses>()])),
            ("tie_break", Value::Null),
        ]);

        // the unanswered format could still provide the eliminations
        assert_eq!(builder.issues(), []);
        assert_eq!(
            builder.finish().err(),
            Some(vec![
                Issue {
                    path: path("format"),
                    problem: Problem::Invalid(ConfigError::Unanswered {
                        path: path("format"),
                    }),
                },
                Issue {
                    path: path("format"),
                    problem: Problem::Unprovided {
                        fact: "eliminations",
                        required_by: path("metrics[0]"),
                        providers: vec!["knockout"],
                    },
                },
            ])
        );
    }

    #[test]
    fn finishing_settles_what_a_single_plugin_provides() {
        let settled = answered(&[
            (
                "format",
                plugin::<Knockout>().with("third_place", false).into(),
            ),
            ("metrics", json!([plugin::<Buchholz>()])),
            ("tie_break", Value::Null),
        ]);

        // wins has a field, so score is the only plugin providing the points that is added
        assert_eq!(
            settled.finish().map(|phase| format!("{:?}", phase.metrics)),
            Ok("[Buchholz, Score, MatchCount]".to_owned())
        );
        // finishing leaves the answers as they were
        assert_eq!(
            settled.answers().get("metrics"),
            Some(&json!([plugin::<Buchholz>()]))
        );
    }

    #[test]
    fn a_built_composite_is_finished_from_what_it_serializes_to() {
        let phase = Phase {
            format: Box::new(Knockout { third_place: true }),
            metrics: vec![Box::new(Buchholz), Box::new(Score)],
            tie_break: None,
        };
        let builder = Builder::from_built(
            Declaration::of(|declaration| {
                declaration.provide(EntrantsPerRanking(1));
            }),
            &phase,
        )
        .unwrap();

        assert_eq!(
            builder.answers().get("format"),
            Some(&json!({"type": "knockout", "value": {"third_place": true}}))
        );
        assert_eq!(
            builder.answers().get("metrics"),
            Some(&json!([plugin::<Buchholz>(), plugin::<Score>()]))
        );
        assert_eq!(
            builder.finish().map(|phase| format!("{phase:?}")),
            Ok(format!(
                "{:?}",
                Phase {
                    format: Box::new(Knockout { third_place: true }),
                    metrics: vec![Box::new(Buchholz), Box::new(Score), Box::new(MatchCount)],
                    tie_break: None,
                }
            ))
        );
    }

    /// Resolution, on axes of its own so that what the catalog settles is not changed by the
    /// plugins of the other tests.
    mod resolution {
        use super::*;

        #[derive(Debug)]
        struct Settled;

        #[derive(Debug)]
        struct Ambiguous;

        #[derive(Debug)]
        struct Downstream;

        #[derive(Debug)]
        struct Deep;

        #[derive(Debug)]
        struct Crosswise;

        #[derive(Debug)]
        struct Lone;

        #[derive(Debug)]
        struct Nested;

        /// The axis of [`Requiring`], taken by a single slot only.
        #[axis]
        trait Need: Plugin {}

        /// An axis taken by lists of the composites.
        #[axis]
        trait Supply: Plugin {}

        /// Another axis taken by a list of a composite.
        #[axis]
        trait Extra: Plugin {}

        /// An axis taken by a list in a plugin only.
        #[axis]
        trait Inner: Plugin {}

        /// A plugin requiring the facts whose fields are set, so that a composite holding it
        /// requires them.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "requiring",
            label = "requiring",
            description = "a plugin requiring what it is told to",
            declare = Self::declare
        )]
        struct Requiring {
            #[serde(default)]
            #[plugin(label = "Settled")]
            settled: bool,
            #[serde(default)]
            #[plugin(label = "Ambiguous")]
            ambiguous: bool,
            #[serde(default)]
            #[plugin(label = "Downstream")]
            downstream: bool,
            #[serde(default)]
            #[plugin(label = "Deep")]
            deep: bool,
            #[serde(default)]
            #[plugin(label = "Crosswise")]
            crosswise: bool,
            #[serde(default)]
            #[plugin(label = "Lone")]
            lone: bool,
            #[serde(default)]
            #[plugin(label = "Nested")]
            nested: bool,
        }

        impl Requiring {
            fn declare(&self, declaration: &mut Declaration) {
                if self.settled {
                    declaration.require::<Settled>("the settled fact");
                }
                if self.ambiguous {
                    declaration.require::<Ambiguous>("the ambiguous fact");
                }
                if self.downstream {
                    declaration.require::<Downstream>("the downstream fact");
                }
                if self.deep {
                    declaration.require::<Deep>("the deep fact");
                }
                if self.crosswise {
                    declaration.require::<Crosswise>("the crosswise fact");
                }
                if self.lone {
                    declaration.require::<Lone>("the lone fact");
                }
                if self.nested {
                    declaration.require::<Nested>("the nested fact");
                }
            }
        }

        #[register]
        impl Need for Requiring {}

        /// The only plugin providing [`Lone`], of the axis only a single slot takes.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "alone",
            label = "alone",
            description = "a plugin of the resolution tests",
            declare_static = Self::declare_static
        )]
        struct Alone;

        impl Alone {
            fn declare_static(declaration: &mut Declaration) {
                declaration.provide(Lone);
            }
        }

        #[register]
        impl Need for Alone {}

        /// The only plugin without fields that provides [`Settled`].
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "settling",
            label = "settling",
            description = "a plugin of the resolution tests",
            declare_static = Self::declare_static
        )]
        struct Settling;

        impl Settling {
            fn declare_static(declaration: &mut Declaration) {
                declaration.provide(Settled);
            }
        }

        #[register]
        impl Supply for Settling {}

        /// A plugin providing [`Settled`] that has to be configured.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "configured_settling",
            label = "configured settling",
            description = "a plugin of the resolution tests",
            declare_static = Self::declare_static
        )]
        struct ConfiguredSettling {
            #[plugin(label = "Rounds")]
            rounds: u32,
        }

        impl ConfiguredSettling {
            fn declare_static(declaration: &mut Declaration) {
                declaration.provide(Settled);
            }
        }

        #[register]
        impl Supply for ConfiguredSettling {}

        fn provides_ambiguous(declaration: &mut Declaration) {
            declaration.provide(Ambiguous);
        }

        /// One of the two plugins providing [`Ambiguous`].
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "one_ambiguous",
            label = "one ambiguous",
            description = "a plugin of the resolution tests",
            declare_static = provides_ambiguous
        )]
        struct OneAmbiguous;

        #[register]
        impl Supply for OneAmbiguous {}

        /// The other plugin providing [`Ambiguous`].
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "other_ambiguous",
            label = "other ambiguous",
            description = "a plugin of the resolution tests",
            declare_static = provides_ambiguous
        )]
        struct OtherAmbiguous;

        #[register]
        impl Supply for OtherAmbiguous {}

        /// The only plugin providing [`Downstream`], requiring what [`Settling`] provides.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "chaining",
            label = "chaining",
            description = "a plugin of the resolution tests",
            declare_static = Self::declare_static
        )]
        struct Chaining;

        impl Chaining {
            fn declare_static(declaration: &mut Declaration) {
                declaration
                    .provide(Downstream)
                    .require::<Settled>("the settled fact");
            }
        }

        #[register]
        impl Supply for Chaining {}

        /// A plugin providing [`Crosswise`], which [`Deepening`] of another axis provides too.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "crossing",
            label = "crossing",
            description = "a plugin of the resolution tests",
            declare_static = Self::declare_static
        )]
        struct Crossing;

        impl Crossing {
            fn declare_static(declaration: &mut Declaration) {
                declaration.provide(Crosswise);
            }
        }

        #[register]
        impl Supply for Crossing {}

        /// A plugin holding a list of plugins, which is never added to.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "bundle",
            label = "bundle",
            description = "a plugin of the resolution tests"
        )]
        struct Bundle {
            #[serde(default)]
            #[plugin(label = "Parts")]
            parts: Vec<Box<dyn Inner>>,
        }

        #[register]
        impl Supply for Bundle {}

        /// The only plugin providing [`Deep`], and one of the two providing [`Crosswise`].
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "deepening",
            label = "deepening",
            description = "a plugin of the resolution tests",
            declare_static = Self::declare_static
        )]
        struct Deepening;

        impl Deepening {
            fn declare_static(declaration: &mut Declaration) {
                declaration.provide(Deep).provide(Crosswise);
            }
        }

        #[register]
        impl Extra for Deepening {}

        /// The only plugin providing [`Nested`], of the axis only [`Bundle`] takes.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "nesting",
            label = "nesting",
            description = "a plugin of the resolution tests",
            declare_static = Self::declare_static
        )]
        struct Nesting;

        impl Nesting {
            fn declare_static(declaration: &mut Declaration) {
                declaration.provide(Nested);
            }
        }

        #[register]
        impl Inner for Nesting {}

        /// A composite taking lists of two axes.
        #[derive(Debug, Serialize, Deserialize, Configurable)]
        struct Resolved {
            #[plugin(label = "Requirements")]
            requirements: Box<dyn Need>,
            #[serde(default)]
            #[plugin(label = "Supplies")]
            supplies: Vec<Box<dyn Supply>>,
            #[serde(default)]
            #[plugin(label = "Extras")]
            extras: Vec<Box<dyn Extra>>,
        }

        /// A composite taking the same axis in two lists.
        #[derive(Debug, Serialize, Deserialize, Configurable)]
        struct Twice {
            #[plugin(label = "Requirements")]
            requirements: Box<dyn Need>,
            #[serde(default)]
            #[plugin(label = "First")]
            first: Vec<Box<dyn Supply>>,
            #[serde(default)]
            #[plugin(label = "Second")]
            second: Vec<Box<dyn Supply>>,
        }

        /// The ids of the plugins in the lists of the composite `C` finished with `context`, its
        /// requirements requiring the facts named in `requires`, its lists empty and `answers`
        /// set, or the facts it reports unprovided, each with the path it reports it at.
        fn finished<C: Configurable + Serialize>(
            context: Declaration,
            requires: &[&str],
            answers: &[(&str, Value)],
        ) -> Result<Vec<String>, Vec<(String, &'static str)>> {
            let requiring = Requiring::schema()
                .fields()
                .iter()
                .fold(plugin::<Requiring>(), |requiring, field| {
                    requiring.with(field.name(), requires.contains(&field.name()))
                });
            let mut builder = Builder::<C>::new(context);
            builder.set(&path("requirements"), requiring).unwrap();
            for field in C::schema().fields() {
                if matches!(field.kind(), FieldKind::List(_)) {
                    builder.set(&path(field.name()), json!([])).unwrap();
                }
            }
            for (at, answer) in answers {
                builder.set(&path(at), answer.clone()).unwrap();
            }
            match builder.finish() {
                Ok(built) => {
                    let built = serde_json::to_value(built).unwrap();
                    Ok(built
                        .as_object()
                        .unwrap()
                        .values()
                        .filter_map(Value::as_array)
                        .flatten()
                        .map(|plugin| plugin["type"].as_str().unwrap().to_owned())
                        .collect())
                }
                Err(issues) => Err(issues
                    .into_iter()
                    .map(|issue| match issue.problem {
                        Problem::Unprovided { fact, .. } => (issue.path.to_string(), fact),
                        problem => panic!("{problem:?} is not about a fact nothing provides"),
                    })
                    .collect()),
            }
        }

        #[test]
        fn the_providers_of_what_an_added_plugin_requires_are_added_too() {
            // a plugin with fields does not make the settled fact ambiguous
            assert_eq!(
                finished::<Resolved>(Declaration::new(), &["downstream"], &[]),
                Ok(vec!["chaining".to_owned(), "settling".to_owned()])
            );
        }

        #[test]
        fn a_provider_is_added_to_the_list_taking_its_axis() {
            assert_eq!(
                finished::<Resolved>(Declaration::new(), &["deep"], &[]),
                Ok(vec!["deepening".to_owned()])
            );
        }

        #[test]
        fn a_fact_plugins_of_two_lists_provide_is_left_unprovided() {
            assert_eq!(
                finished::<Resolved>(Declaration::new(), &["crosswise"], &[]),
                Err(vec![
                    ("supplies".to_owned(), "the crosswise fact"),
                    ("extras".to_owned(), "the crosswise fact"),
                ])
            );
        }

        #[test]
        fn nothing_is_added_for_a_fact_the_context_provides() {
            let context = Declaration::of(|declaration| {
                declaration.provide(Settled);
            });

            assert_eq!(
                finished::<Resolved>(context, &["settled"], &[]),
                Ok(Vec::new())
            );
        }

        #[test]
        fn an_ambiguous_fact_is_left_unprovided_and_the_others_settled() {
            assert_eq!(
                finished::<Resolved>(Declaration::new(), &["ambiguous", "settled"], &[]),
                Err(vec![("supplies".to_owned(), "the ambiguous fact")])
            );
        }

        #[test]
        fn a_single_slot_is_never_added_to() {
            assert_eq!(
                finished::<Resolved>(Declaration::new(), &["lone"], &[]),
                Err(vec![("requirements".to_owned(), "the lone fact")])
            );
        }

        #[test]
        fn a_list_of_a_plugin_is_never_added_to() {
            assert_eq!(
                finished::<Resolved>(
                    Declaration::new(),
                    &["nested"],
                    &[(
                        "supplies",
                        json!([plugin::<Bundle>().with("parts", json!([]))])
                    )]
                ),
                Err(vec![
                    ("supplies".to_owned(), "the nested fact"),
                    ("supplies[0].parts".to_owned(), "the nested fact"),
                ])
            );
        }

        #[test]
        fn an_axis_two_lists_take_is_never_added_to() {
            assert_eq!(
                finished::<Twice>(Declaration::new(), &["settled"], &[]),
                Err(vec![
                    ("first".to_owned(), "the settled fact"),
                    ("second".to_owned(), "the settled fact"),
                ])
            );
        }
    }
}
