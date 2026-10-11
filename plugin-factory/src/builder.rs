//! A composite of plugins being configured, one answer at a time.
//!
//! A [`Builder`] holds the answers given so far to the schema of a composite and to the schemas
//! of the plugins answering its fields, as one tree in which a [`Path`] addresses a field or a
//! position in a list. A UI renders from its queries — the [questions](Builder::questions) the
//! tree asks, the [default](Builder::default) of a field, the [candidates](Builder::candidates)
//! for a position taking plugins and the [issues](Builder::issues) of the answers so far — and
//! changes the tree through [`Builder::set`] and [`Builder::unset`] alone, so that no answer is
//! ever changed behind its back. [`Builder::finish`] builds the composite.
//!
//! While a field taking plugins is unanswered, whatever the plugins of its axis could provide may
//! still arrive, see [`OpenFacts`], so the candidates and issues are optimistic about it;
//! [`Builder::finish`] checks with nothing open.

use std::{fmt::Write as _, marker::PhantomData, mem};

use thiserror::Error;

use crate::{
    catalog::{AnyAxis, PluginEntry},
    compatibility::{self, Declaration, Incompatibility, OpenFacts},
    config::{self, Config, ConfigError, Path, PluginConfig, Segment, Value},
    schema::{Configurable, Field, FieldKind, Schema},
};

/// The answers to the schema of the composite `T` given so far, see the [module](self).
pub struct Builder<T> {
    context: Declaration,
    answers: Config,
    composite: PhantomData<fn() -> T>,
}

impl<T: Configurable> Builder<T> {
    /// A builder with nothing answered, for a composite with `context`, i.e. what is true of it
    /// before any plugin is chosen, see [`compatibility::check_facts`].
    pub fn new(context: Declaration) -> Self {
        Self {
            context,
            answers: Config::new(),
            composite: PhantomData,
        }
    }

    /// The answers given so far, as the config of the composite.
    pub fn answers(&self) -> &Config {
        &self.answers
    }

    /// Every field of the composite and of the plugins answering its fields, answered or not,
    /// depth first in schema order: a field taking plugins is followed by the fields of the
    /// plugins answering it.
    pub fn questions(&self) -> Vec<Question> {
        Tree::of(&T::schema(), &self.answers).questions
    }

    /// The answer the field at `path` offers, or [`None`] if it offers none or `path` addresses a
    /// position in a list rather than a field.
    ///
    /// A field taking plugins offers what the plugins answering the other fields of its config
    /// recommend, see [`Plugin::recommends`](crate::Plugin::recommends), as far as it is of its
    /// axis and fits as a [candidate](Self::candidates) would: a field taking one plugin the first
    /// such recommendation, a field taking a list all of them that fit together, in the order they
    /// are recommended in. A field no recommendation fits offers the default of its schema.
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers.
    pub fn default(&self, path: &Path) -> Result<Option<Value>, ConfigError> {
        self.default_in(&self.answers, path)
    }

    /// The plugins that could answer the position at `path`, by id, each with why it does not fit
    /// if it does not; empty if the position takes no plugins.
    ///
    /// A candidate takes the place of what answers the position, so it is checked against the
    /// answers without it, see [`compatibility::check_addition`]. What the fields still
    /// unanswered could provide is open, and so is what the candidate's own plugin fields could.
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
        let tree = Tree::of(&schema, &self.answers).without(path);
        let declarations = tree.declarations();
        let mut open = tree.open();
        if list {
            open.extend(axis.facts());
        }
        let mut candidates: Vec<_> = axis
            .entries()
            .map(|entry| {
                let mut open = open.clone();
                open.extend(entry.sub_axes().iter().flat_map(AnyAxis::facts));
                let conflict = compatibility::check_addition(
                    &self.context,
                    &declarations,
                    (entry.label(), &entry.declaration()),
                    &open,
                )
                .err();
                Candidate { entry, conflict }
            })
            .collect();
        candidates.sort_by_key(|candidate| candidate.entry.id());
        Ok(candidates)
    }

    /// Everything currently wrong with the answers, each at the path where it is fixed, in no
    /// particular order: an answer a plugin rejects, a plugin that does not fit next to the
    /// others, and a fact a plugin requires that nothing provides and no unanswered field could.
    ///
    /// A field without a default left unanswered is no issue, but a question.
    pub fn issues(&self) -> Vec<Issue> {
        let tree = Tree::of(&T::schema(), &self.answers);
        let mut issues = Vec::new();
        for placed in &tree.plugins {
            match placed.entry.instance(&placed.config) {
                Ok(_) | Err(ConfigError::Unanswered { .. }) => {}
                Err(error) => {
                    let error = placed.path.prefix(error);
                    let issue = Issue {
                        path: error.path().clone(),
                        problem: Problem::Invalid(error),
                    };
                    // a plugin is built with the plugins answering its fields, so an error of
                    // one of those is found again building the plugin holding it
                    if !issues.contains(&issue) {
                        issues.push(issue);
                    }
                }
            }
        }

        let mut declarations = tree.declarations();
        let mut open = tree.open();
        let mut unprovided = Vec::new();
        for ((_, declaration), placed) in declarations.iter().zip(&tree.plugins) {
            for requirement in declaration.required() {
                let fact = requirement.type_id();
                if !self.context.provides_type(fact)
                    && !declarations
                        .iter()
                        .any(|(_, declaration)| declaration.provides_type(fact))
                    && !open.contains(fact)
                {
                    unprovided.push((fact, requirement.name(), placed.path.clone()));
                }
            }
        }
        // a requirement nothing provides is reported below, at the fields that could provide it
        open.extend(unprovided.iter().map(|&(fact, _, _)| fact));
        for (index, placed) in tree.plugins.iter().enumerate() {
            let (name, declaration) = declarations.remove(index);
            if let Err(incompatibility) = compatibility::check_addition(
                &self.context,
                &declarations,
                (&name, &declaration),
                &open,
            ) {
                issues.push(Issue {
                    path: placed.path.clone(),
                    problem: Problem::Incompatible(incompatibility),
                });
            }
            declarations.insert(index, (name, declaration));
        }

        for (fact, name, required_by) in unprovided {
            let fields: Vec<_> = tree
                .questions
                .iter()
                .filter(|question| question.answer.is_some())
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
                let mut providers: Vec<_> = axis
                    .entries()
                    .filter(|entry| entry.declaration().provides_type(fact))
                    .map(PluginEntry::id)
                    .collect();
                providers.sort_unstable();
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
    /// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers, and
    /// otherwise the errors of [`Schema::complete`] other than [`ConfigError::Unanswered`], with
    /// paths seen from the root. Nothing is changed then.
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

    /// The composite, built from the answers with every unanswered field answered by its
    /// [default](Self::default), as if an operator had accepted each one in the order of
    /// [`Self::questions`].
    ///
    /// Leaves the builder as it is, so that a failed composite can be fixed and finished again.
    ///
    /// # Errors
    /// Returns [`FinishError::Config`] if a field without a default is unanswered or an answer or
    /// default is rejected, and [`FinishError::Incompatible`] if the plugins do not fit together
    /// with nothing open any more, see [`compatibility::check_facts`].
    pub fn finish(&self) -> Result<T, FinishError> {
        let schema = T::schema();
        let mut answers = self.answers.clone();
        while let Some((path, default)) = Tree::of(&schema, &answers)
            .questions
            .into_iter()
            .filter(|question| question.answer.is_none())
            .find_map(|question| {
                let default = self.default_in(&answers, &question.path).ok()??;
                Some((question.path, default))
            })
        {
            set_in(&schema, &mut answers, &path, &default)?;
        }
        let completed = schema.complete(&answers)?;
        let declarations = Tree::of(&schema, &completed)
            .plugins
            .iter()
            .map(Placed::configured_declaration)
            .collect::<Result<Vec<_>, _>>()?;
        compatibility::check_facts(&self.context, &declarations, &OpenFacts::NOTHING)?;
        Ok(config::deserialize(&completed.into())?)
    }

    /// [`Self::default`] with `answers` as the answers.
    fn default_in(&self, answers: &Config, path: &Path) -> Result<Option<Value>, ConfigError> {
        let schema = T::schema();
        let position = locate(&schema, answers, path)?;
        let Some(field) = position.field else {
            return Ok(None);
        };
        let Some((axis, list)) = slot(field.kind()) else {
            return Ok(field.default_value().cloned());
        };
        let tree = Tree::of(&schema, answers);
        let parent = path.parent();
        let recommended: Vec<_> = tree
            .plugins
            .iter()
            .filter(|placed| placed.parent == parent && !placed.path.starts_with(path))
            .filter_map(|placed| placed.entry.instance(&placed.config).ok())
            .flat_map(|instance| instance.recommends())
            .filter_map(|recommended| Some((axis.entry(recommended.plugin())?, recommended)))
            .collect();

        let tree = tree.without(path);
        let mut declarations = tree.declarations();
        let mut open = tree.open();
        if list {
            open.extend(axis.facts());
        }
        let mut fitting = Vec::new();
        for (entry, recommended) in recommended {
            // a recommendation is a whole answer, so what it declares includes the plugins
            // answering its fields
            let declaration = entry
                .declaration_for(recommended.config())
                .unwrap_or_else(|_| entry.declaration());
            let name = entry.label();
            if compatibility::check_addition(
                &self.context,
                &declarations,
                (name, &declaration),
                &open,
            )
            .is_ok()
            {
                declarations.push((name.to_owned(), declaration));
                fitting.push(Value::from(recommended));
                if !list {
                    break;
                }
            }
        }
        Ok(match (list, fitting.is_empty()) {
            (_, true) => field.default_value().cloned(),
            (true, false) => Some(Value::Array(fitting)),
            (false, false) => fitting.pop(),
        })
    }
}

/// One field of the tree of answers, see [`Builder::questions`].
#[derive(Debug, Clone)]
pub struct Question {
    path: Path,
    field: Field,
    answer: Option<Value>,
}

impl Question {
    /// Where the field is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The field, i.e. what it is called and what kind of answer it takes.
    pub fn field(&self) -> &Field {
        &self.field
    }

    /// The answer the field holds, or [`None`] if it is unanswered.
    pub fn answer(&self) -> Option<&Value> {
        self.answer.as_ref()
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

    /// Whether the plugin fits.
    pub fn fits(&self) -> bool {
        self.conflict.is_none()
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
    /// could. The issue is at each answered field whose axis could provide it, with the ids of the
    /// plugins of that axis that do, or at `required_by` if there is none.
    Unprovided {
        fact: &'static str,
        required_by: Path,
        providers: Vec<&'static str>,
    },
}

/// Why a [`Builder`] cannot build its composite, see [`Builder::finish`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FinishError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Incompatible(#[from] Incompatibility),
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
    let answer = position
        .kind
        .check(answer)
        .map_err(|error| path.prefix(error))?;
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

/// The answers as a tree: every field and every plugin answering one.
struct Tree {
    /// Every field, depth first in schema order.
    questions: Vec<Question>,
    /// Every plugin answering a field or a position in a list, depth first in schema order.
    plugins: Vec<Placed>,
}

/// A plugin in the answers.
struct Placed {
    path: Path,
    /// The path of the config whose field the plugin answers.
    parent: Path,
    entry: &'static PluginEntry,
    config: Config,
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
            let path = at.clone().then(Segment::field(field.name()));
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
                    self.add_answer(
                        element,
                        answer,
                        path.clone().then(Segment::Index(index)),
                        parent,
                    );
                }
            }
            (FieldKind::Plugin(axis), _) => {
                let Some(plugin) = PluginConfig::from_answer(answer) else {
                    return;
                };
                let Some(entry) = axis.entry(plugin.plugin()) else {
                    return;
                };
                self.plugins.push(Placed {
                    path: path.clone(),
                    parent: parent.clone(),
                    entry,
                    config: plugin.config().clone(),
                });
                self.add_config(&entry.schema(), plugin.config(), &path);
            }
            _ => {}
        }
    }

    /// This tree without what is at `path` and within it.
    fn without(mut self, path: &Path) -> Self {
        self.questions
            .retain(|question| !question.path.starts_with(path));
        self.plugins.retain(|placed| !placed.path.starts_with(path));
        self
    }

    /// The types of the facts the fields still unanswered could provide.
    fn open(&self) -> OpenFacts {
        let mut axes = Vec::new();
        for question in &self.questions {
            if question.answer.is_none()
                && let Some((axis, _)) = slot(question.field.kind())
                && !axes.contains(&axis)
            {
                axes.push(axis);
            }
        }
        axes.iter().flat_map(AnyAxis::facts).collect()
    }

    /// What the plugins declare, each named as the checks report it, in the order of
    /// [`Self::plugins`].
    fn declarations(&self) -> Vec<(String, Declaration)> {
        self.plugins.iter().map(Placed::declaration).collect()
    }
}

impl Placed {
    /// What the plugin declares itself, without the plugins answering its fields, which are
    /// placed in the tree themselves: what its configuration declares if its config completes and
    /// is accepted, and its static declaration otherwise.
    fn declaration(&self) -> (String, Declaration) {
        let mut declaration = self.entry.declaration();
        if let Ok(instance) = self.entry.instance(&self.config) {
            instance.declare(&mut declaration);
        }
        (self.name(), declaration)
    }

    /// [`Self::declaration`] of a plugin whose config has to complete and be accepted.
    fn configured_declaration(&self) -> Result<(String, Declaration), ConfigError> {
        let instance = self
            .entry
            .instance(&self.config)
            .map_err(|error| self.path.prefix(error))?;
        let mut declaration = self.entry.declaration();
        instance.declare(&mut declaration);
        Ok((self.name(), declaration))
    }

    /// What the plugin is called in an incompatibility.
    fn name(&self) -> String {
        format!("{} ({})", self.entry.label(), self.path)
    }
}

/// The builder, tested on axes of its own so that it does not depend on what the plugins of a
/// program happen to register.
#[cfg(test)]
mod tests {
    use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
    use serde_json::json;

    use super::*;
    use crate::{Plugin, PluginType, axis, compatibility::Condition, register};

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
                PluginConfig::of::<MatchCount>(Config::new()),
                PluginConfig::of::<Wins>(Config::new()),
                PluginConfig::of::<Score>(Config::new()),
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
            declaration.require(
                "the entrants per ranking",
                Some(Condition::new(
                    "rankings of single players",
                    |entrants: &EntrantsPerRanking| entrants.0 == 1,
                )),
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
                .require::<Points>("points", None)
                .require::<Matches>("matches", None);
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
            declaration.require::<Eliminating>("eliminations", None);
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
    #[derive(Debug, Deserialize)]
    struct Phase {
        #[expect(dead_code, reason = "read through Debug")]
        format: Box<dyn Format>,
        #[expect(dead_code, reason = "read through Debug")]
        metrics: Vec<Box<dyn Metric>>,
        #[expect(dead_code, reason = "read through Debug")]
        tie_break: Option<Box<dyn Metric>>,
    }

    impl Configurable for Phase {
        fn schema() -> Schema {
            Schema::new()
                .field(Field::plugin::<dyn Format>("format", "Format"))
                .field(Field::plugins::<dyn Metric>("metrics", "Metrics").with_default(json!([])))
                .field(
                    Field::plugin::<dyn Metric>("tie_break", "Tie break")
                        .optional()
                        .with_default(Value::Null),
                )
        }
    }

    /// A builder of a phase of single players.
    fn builder() -> Builder<Phase> {
        Builder::new(Declaration::of(|declaration| {
            declaration.provide(EntrantsPerRanking(1));
        }))
    }

    /// The path written as `text`, e.g. `metrics[1].for_win`.
    fn path(text: &str) -> Path {
        let mut path = Path::ROOT;
        for part in text.split('.') {
            let mut pieces = part.split('[');
            path = path.then(Segment::field(pieces.next().unwrap()));
            for index in pieces {
                path = path.then(Segment::Index(index.trim_end_matches(']').parse().unwrap()));
            }
        }
        path
    }

    /// The plugin `P` with nothing answered.
    fn plugin<P: PluginType>() -> PluginConfig {
        PluginConfig::of::<P>(Config::new())
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

    #[test]
    fn the_questions_are_the_fields_of_the_composite_and_of_the_plugins_answering_them() {
        let paths = |builder: &Builder<Phase>| -> Vec<(String, bool)> {
            builder
                .questions()
                .iter()
                .map(|question| (question.path().to_string(), question.answer().is_some()))
                .collect()
        };
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
                json!([PluginConfig::of::<Wins>(
                    Config::new().with("for_win", true)
                )])
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

        assert!(
            builder
                .questions()
                .iter()
                .any(|question| question.path() == &path("metrics[0].parts"))
        );
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
        let builder = answered(&[(
            "metrics",
            json!([PluginConfig::of::<Wins>(
                Config::new().with("for_win", -1.0)
            )]),
        )]);
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

    #[test]
    fn finishing_accepts_every_default_in_the_order_the_questions_are_asked_in() {
        let builder = answered(&[("format", plugin::<RoundRobin>().into())]);

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
        assert_eq!(builder.answers().get("metrics"), None);
    }

    #[test]
    fn finishing_with_a_field_without_a_default_unanswered_fails() {
        assert_eq!(
            builder().finish().err(),
            Some(FinishError::Config(ConfigError::Unanswered {
                path: path("format"),
            }))
        );
    }

    #[test]
    fn finishing_with_an_answer_a_plugin_rejects_fails() {
        let builder = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            (
                "metrics",
                json!([PluginConfig::of::<Wins>(
                    Config::new().with("for_win", -1.0)
                )]),
            ),
        ]);

        assert_eq!(
            builder.finish().err(),
            Some(FinishError::Config(ConfigError::Rejected {
                path: path("metrics[0].for_win"),
                reason: "the points for a win cannot be negative".to_owned(),
            }))
        );
    }

    #[test]
    fn finishing_checks_with_nothing_open() {
        let builder = answered(&[
            ("format", plugin::<Knockout>().into()),
            ("metrics", json!([plugin::<Buchholz>()])),
        ]);

        assert_eq!(
            builder.finish().err(),
            Some(FinishError::Incompatible(
                Incompatibility::FactNotProvided {
                    fact: "points",
                    required_by: "buchholz (metrics[0])".to_owned(),
                }
            ))
        );
    }
}
