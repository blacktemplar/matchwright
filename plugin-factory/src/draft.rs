//! A composite of plugins being configured, one answer at a time.
//!
//! A [`Draft`] holds the answers given so far to the schema of a composite and to the schemas of
//! the plugins answering its fields, as one tree in which a [`Path`] addresses a field or a
//! position in a list. A UI renders the fields from the schemas along the
//! [answers](Draft::answers), see [`Draft`], and changes the tree through [`Draft::set`] and
//! [`Draft::unset`] alone, so that every answer in it fits the schema.
//!
//! The functions of this module answer what a UI asks about a draft: the
//! [default](default_value) of a field, the [candidates] for a position taking plugins and the
//! [issues] of the answers so far. [`finish`] builds the composite. Each takes the composite's
//! context, i.e. what is true of it before any plugin is chosen, as a closure declaring it as a
//! plugin declares, see [`compatibility::check`].
//!
//! While a field taking plugins is unanswered, a fact a plugin of its axis provides may still
//! arrive, as long as such a plugin would fit next to the answers given and what it requires could
//! arrive in turn, so the candidates and issues are optimistic about it; [`finish`] checks with
//! nothing open. Whether a provider fits is judged by its static declaration.

use std::{
    any::TypeId,
    fmt::{self, Display, Formatter},
    marker::PhantomData,
};

use serde::{Deserialize, Serialize};

use crate::{
    Plugin,
    catalog::{AnyAxis, PluginEntry},
    compatibility::{
        self, Declaration, Declared, Incompatibility, OpenFacts, ProvidedFact, RequiredFact,
    },
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
///     config::{Config, Path, PluginConfig, Value},
///     draft::Draft,
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
///     #[plugin(label = "Keep the top seeds", default = 0)]
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
///         (FieldKind::List { element, .. }, Value::Array(answers)) => {
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
/// let mut draft = Draft::<Phase>::new();
/// draft
///     .set(
///         &"seedings".parse().unwrap(),
///         vec![PluginConfig::of::<Random>()],
///     )
///     .unwrap();
///
/// let mut lines = Vec::new();
/// fields(&Phase::schema(), draft.answers(), &Path::ROOT, &mut lines);
/// assert_eq!(
///     lines,
///     [
///         "seedings: Seedings",
///         "seedings[0].keep_top: Keep the top seeds"
///     ]
/// );
/// # }
/// ```
pub struct Draft<T> {
    answers: Config,
    composite: PhantomData<fn() -> T>,
}

impl<T: Configurable> Draft<T> {
    /// A draft with nothing answered.
    pub fn new() -> Self {
        Self {
            answers: Config::new(),
            composite: PhantomData,
        }
    }

    /// A draft with every field answered as in `composite`, e.g. one that was loaded; the answers
    /// are what `composite` serializes to. So [`finish`] of it settles and checks a composite
    /// built elsewhere.
    ///
    /// # Errors
    /// Returns [`ConfigError::Rejected`] at the root if `composite` does not serialize to a
    /// config, and what [`Self::set`] returns for an answer that is not one of the schema.
    pub fn from_built(composite: &T) -> Result<Self, ConfigError>
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
            answers: T::schema().walk(&answers, &Path::ROOT, false)?,
            composite: PhantomData,
        })
    }

    /// The answers given so far, as the config of the composite.
    pub fn answers(&self) -> &Config {
        &self.answers
    }

    /// Answers the position at `path` with `answer`: a field, a position in a list, or the
    /// position just past its end, which appends `answer` to the list.
    ///
    /// The answer may leave fields of the plugins in it unanswered. What a plugin rejects is not
    /// checked here but reported as an [issue](issues).
    ///
    /// # Errors
    /// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers or the
    /// answer answers a field a schema does not have, [`ConfigError::WrongKind`] if an answer is
    /// not one its field takes, [`ConfigError::UnknownPlugin`] if a plugin in it is not
    /// registered for the axis of its field, and [`ConfigError::Duplicate`] if a plugin it puts
    /// into a list is in that list already, with paths seen from the root. Nothing is changed
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
}

impl<T: Configurable> Default for Draft<T> {
    fn default() -> Self {
        Self::new()
    }
}

// written out rather than derived, which would demand `T: Clone` of the composite
impl<T> Clone for Draft<T> {
    fn clone(&self) -> Self {
        Self {
            answers: self.answers.clone(),
            composite: PhantomData,
        }
    }
}

/// The answer the field at `path` of `draft` offers in a composite with `context`, or [`None`] if
/// it offers none or `path` addresses a position in a list rather than a field. It is never filled
/// in: a UI offering it sets it as the answer, see [`Draft::set`].
///
/// A field taking plugins offers what the plugins answering the other fields of its config
/// recommend, see [`Plugin::recommends`], as far as it is of its axis and fits as a
/// [candidate](candidates) would: a field taking one plugin the first such recommendation, a field
/// taking a list all of them that fit together, in the order they are recommended in. A field no
/// recommendation fits offers the default of its schema.
///
/// # Errors
/// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers.
pub fn default_value<T: Configurable>(
    context: impl FnOnce(&mut Declaration),
    draft: &Draft<T>,
    path: &Path,
) -> Result<Option<Value>, ConfigError> {
    let schema = T::schema();
    let Some(field) = locate(&schema, &draft.answers, path)?.field else {
        return Ok(None);
    };
    Ok(default_in(
        &Declaration::of(context),
        &Tree::of(&schema, &draft.answers),
        &field,
        path,
    ))
}

/// The plugins that could answer the position at `path` of `draft` in a composite with `context`,
/// in the order of their ids, each with how it fits; empty if the position takes no plugins.
///
/// A candidate takes the place of what answers the position, so it is checked against the answers
/// without it, and only for what it declares itself: an incompatibility among the other answers is
/// not its conflict. For a field taking a list the candidate is one of the plugins answering it,
/// so the rest of the list is left out as well. For a position in a list, a plugin answering
/// another position of that list is no candidate.
///
/// A fact the candidate requires and nothing provides is
/// - [missing](Candidate::missing) if a field could still take a plugin providing it that fits: a
///   field still unanswered, a list of the composite, or the list the candidate is put into. A
///   provider fits if it fits next to the answers and the candidate, and every fact it requires
///   that nothing provides could arrive in the same way;
/// - not reported at all if one of the candidate's own plugin fields could provide it, or a plugin
///   of an axis no field of the composite takes; [`issues`] and [`finish`] report it once the
///   candidate is chosen;
/// - otherwise, if such a field could take a provider but none fits, a
///   [conflict](Candidate::conflicts) with the answered field whose plugin keeps the providers
///   out, or with nothing if no plugin does;
/// - otherwise a conflict with each answered field of the composite taking one plugin that could
///   provide it, or not reported if there is none.
///
/// # Errors
/// Returns [`ConfigError::UnknownField`] if `path` addresses nothing in the answers. A path may
/// address the position just past the end of a list.
pub fn candidates<T: Configurable>(
    context: impl FnOnce(&mut Declaration),
    draft: &Draft<T>,
    path: &Path,
) -> Result<Vec<Candidate>, ConfigError> {
    let schema = T::schema();
    let position = locate(&schema, &draft.answers, path)?;
    let Some((axis, list)) = slot(&position.kind) else {
        return Ok(Vec::new());
    };
    let context = Declaration::of(context);
    let tree = Tree::of(&schema, &draft.answers);
    let sources = Sources::of(&schema, &tree, path, list.then_some(axis));
    let others: Vec<_> = tree
        .plugins
        .iter()
        .filter(|placed| !is_within(&placed.path, Some(path)))
        .collect();
    let taken: Vec<_> = match path.segments().last() {
        Some(Segment::Index(_)) => {
            let list = path.parent();
            others
                .iter()
                .filter(|placed| placed.path.parent() == list)
                .map(|placed| placed.id)
                .collect()
        }
        _ => Vec::new(),
    };
    Ok(axis
        .entries()
        .filter(|entry| !taken.contains(&entry.id()))
        .map(|entry| candidate(&context, &others, &sources, entry))
        .collect())
}

/// The plugin `entry` as a [candidate](candidates) next to the plugins `others` in a composite
/// with `context`, with the facts of the composite found in `sources`.
fn candidate(
    context: &Declaration,
    others: &[&Placed],
    sources: &Sources,
    entry: &'static PluginEntry,
) -> Candidate {
    let provider = |fact: TypeId| {
        others
            .iter()
            .find(|placed| placed.declared.declaration.provides_type(fact))
            .map(|placed| placed.path.clone())
    };
    let own_facts: OpenFacts = entry
        .sub_axes()
        .iter()
        .flat_map(|axis| axis.facts().iter())
        .collect();
    let declared = Declared::new(entry.label(), entry.declaration());
    // what finishing adds is settled with the candidate chosen
    let with_candidate: Vec<_> = others
        .iter()
        .map(|placed| &placed.declared)
        .chain([&declared])
        .collect();
    let reach = Reach {
        context,
        open: &sources.open,
        answered: &sources.answered,
        placed: others,
    };
    let mut conflicts = Vec::new();
    let mut missing = Vec::new();
    for conflict in compatibility::conflicts(
        context,
        others.iter().map(|placed| &placed.declared),
        &declared,
        &OpenFacts::NOTHING,
    ) {
        let fact = conflict.fact;
        let with = match conflict.incompatibility {
            Incompatibility::FactNotProvided { fact: name, .. } => {
                let blocker = match reach.fields(&with_candidate, fact) {
                    Some(Ok(fields)) => {
                        missing.push(Missing {
                            fact: name,
                            fields,
                            added_by_finish: sources.additions(context, &with_candidate, fact),
                        });
                        continue;
                    }
                    Some(Err(blocker)) => Some(blocker),
                    None => None,
                };
                if own_facts.contains(fact) || sources.provided_nested(fact) {
                    continue;
                }
                // a field could take a provider, but none of them fits
                if let Some(blocker) = blocker {
                    conflicts.push(Conflict {
                        incompatibility: conflict.incompatibility,
                        with: blocker,
                    });
                    continue;
                }
                for field in sources.answered_to(fact) {
                    conflicts.push(Conflict {
                        incompatibility: conflict.incompatibility.clone(),
                        with: Some(field),
                    });
                }
                continue;
            }
            // the provider is the context, or the candidate itself if no other plugin is
            Incompatibility::FactProvidedTwice { .. } | Incompatibility::ConditionNotMet { .. } => {
                provider(fact)
            }
            _ => None,
        };
        conflicts.push(Conflict {
            incompatibility: conflict.incompatibility,
            with,
        });
    }

    let supplies = declared
        .declaration
        .provided()
        .iter()
        .map(ProvidedFact::type_id)
        .filter(|&fact| !context.provides_type(fact) && provider(fact).is_none())
        .filter_map(|fact| {
            let requiring: Vec<_> = others
                .iter()
                .filter_map(|placed| {
                    let requirement = placed
                        .declared
                        .declaration
                        .required()
                        .iter()
                        .find(|requirement| requirement.type_id() == fact)?;
                    Some((requirement.name(), placed.path.clone()))
                })
                .collect();
            let (name, _) = requiring.first()?;
            Some(Supply {
                fact: name,
                required_by: requiring.iter().map(|(_, path)| path.clone()).collect(),
                other_providers: sources.providers_of(fact, entry.id()),
            })
        })
        .collect();

    Candidate {
        entry,
        conflicts,
        missing,
        supplies,
    }
}

/// Where the facts a [candidate](candidates) requires could come from.
struct Sources {
    /// The fields a plugin could still be added to, each with its axis: the fields still
    /// unanswered, the lists of the composite and the list the candidate is put into.
    open: Vec<(Path, AnyAxis)>,
    /// The answered fields of the composite taking one plugin, each with its axis.
    answered: Vec<(Path, AnyAxis)>,
    /// The lists of the composite, as [`settle`] adds to them.
    lists: Vec<(&'static str, AnyAxis)>,
    /// The axes the fields of the composite take.
    top: Vec<AnyAxis>,
    /// Every plugin of an axis reachable from the composite, by axis, each with the types of the
    /// facts it provides itself.
    providers: Vec<(AnyAxis, &'static str, Vec<TypeId>)>,
}

impl Sources {
    /// The sources in `tree`, the tree of the answers to `schema`, for a candidate at `path`, a
    /// position in a list of `list` if it is one.
    fn of(schema: &Schema, tree: &Tree, path: &Path, list: Option<AnyAxis>) -> Self {
        let mut open = Vec::new();
        let mut answered = Vec::new();
        for question in &tree.questions {
            let Some((axis, is_list)) = slot(question.field.kind()) else {
                continue;
            };
            if is_within(&question.path, Some(path)) {
                continue;
            }
            let top_level = question.path.segments().len() == 1;
            if question.answer.is_none() || (top_level && is_list) {
                open.push((question.path.clone(), axis));
            } else if top_level {
                answered.push((question.path.clone(), axis));
            }
        }
        if let Some(axis) = list {
            let list_path = match path.segments().last() {
                Some(Segment::Index(_)) => path.parent().expect("an index is within a list"),
                _ => path.clone(),
            };
            if !open.iter().any(|(open, _)| *open == list_path) {
                open.push((list_path, axis));
            }
        }

        let lists = top_lists(schema);
        let top: Vec<_> = schema
            .fields()
            .iter()
            .filter_map(|field| field.kind().axis())
            .collect();
        let mut reachable = Vec::new();
        let mut pending = top.clone();
        while let Some(axis) = pending.pop() {
            if reachable.contains(&axis) {
                continue;
            }
            reachable.push(axis);
            pending.extend(axis.entries().flat_map(|entry| entry.sub_axes()));
        }
        let providers = reachable
            .iter()
            .flat_map(|axis| {
                axis.entries().map(move |entry| {
                    let provided = entry
                        .declaration()
                        .provided()
                        .iter()
                        .map(ProvidedFact::type_id)
                        .collect();
                    (*axis, entry.id(), provided)
                })
            })
            .collect();
        Self {
            open,
            answered,
            lists,
            top,
            providers,
        }
    }

    /// The answered fields of the composite taking one plugin that could provide `fact`.
    fn answered_to(&self, fact: TypeId) -> Vec<Path> {
        Self::able(&self.answered, fact)
    }

    /// The paths among `fields` whose axis could provide `fact`.
    fn able(fields: &[(Path, AnyAxis)], fact: TypeId) -> Vec<Path> {
        fields
            .iter()
            .filter(|(_, axis)| axis.facts().contains(fact))
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Whether a plugin of an axis no field of the composite takes provides `fact`.
    fn provided_nested(&self, fact: TypeId) -> bool {
        self.providers
            .iter()
            .any(|(axis, _, provided)| !self.top.contains(axis) && provided.contains(&fact))
    }

    /// The ids of the plugins providing `fact` but the one with the id `except`, each once.
    fn providers_of(&self, fact: TypeId, except: &str) -> Vec<&'static str> {
        let mut ids = Vec::new();
        for &(_, id, ref provided) in &self.providers {
            if id != except && provided.contains(&fact) && !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    /// What [`finish`] adds for `fact` in a composite with `context` and plugins with
    /// `declarations`, see [`chain`].
    fn additions(
        &self,
        context: &Declaration,
        declarations: &[&Declared],
        fact: TypeId,
    ) -> Vec<Addition> {
        additions(context, declarations, &self.lists, fact)
    }
}

/// Everything currently wrong with the answers of `draft` in a composite with `context`, each at
/// the path where it is fixed, in no particular order: an answer a plugin rejects, a plugin that
/// does not fit next to the others, and a fact a plugin requires that nothing provides and no
/// unanswered field could take a fitting provider for, as [`candidates`] judges a provider.
///
/// An unanswered field is no issue, but still to be answered. A plugin with an unanswered field
/// counts with its static declaration only and recommends nothing.
pub fn issues<T: Configurable>(
    context: impl FnOnce(&mut Declaration),
    draft: &Draft<T>,
) -> Vec<Issue> {
    let schema = T::schema();
    issues_in(
        &Declaration::of(context),
        &Tree::of(&schema, &draft.answers),
        Some(&top_lists(&schema)),
    )
}

/// The composite `draft` is a draft of, in a composite with `context`, built from its answers
/// with every fact settled that the catalog settles.
///
/// Every field has to be answered; defaults are not filled in. A fact a plugin requires and
/// neither the plugins nor the context provide is settled if exactly one plugin without fields
/// provides it among the axes of the fields of the composite taking a list of plugins, and its
/// axis is taken by one such field only: the plugin is appended to that field. What it requires
/// that nothing provides is settled in turn, and the plugins so found are appended only all
/// together and only if none of them raises an issue: a conflict, a fact missing, or a condition
/// an existing plugin requires of what they provide. A field taking one plugin, and a field of a
/// plugin, is never added to. This repeats until nothing changes.
///
/// Leaves `draft` as it is, so that a failed composite can be fixed and finished again.
///
/// # Errors
/// Returns the [issues] of the answers so settled, with nothing open any more: an unanswered field
/// is a [`Problem::Invalid`] issue with [`ConfigError::Unanswered`], and a fact nothing provides
/// is [`Problem::Unprovided`].
pub fn finish<T: Configurable>(
    context: impl FnOnce(&mut Declaration),
    draft: &Draft<T>,
) -> Result<T, Vec<Issue>> {
    let context = Declaration::of(context);
    let schema = T::schema();
    let invalid = |error: ConfigError| {
        vec![Issue {
            path: error.path().clone(),
            problem: Problem::Invalid(error),
        }]
    };
    let mut answers = draft.answers.clone();
    let mut tried = Vec::new();
    while settle(&context, &schema, &mut answers, &mut tried) {}
    let issues = issues_in(&context, &Tree::of(&schema, &answers), None);
    if !issues.is_empty() {
        return Err(issues);
    }
    let walked = schema.walk(&answers, &Path::ROOT, true).map_err(invalid)?;
    config::deserialize(&config::complete::<T>(walked), &Path::ROOT).map_err(invalid)
}

/// Whether a fact nothing provides yet could still arrive: only if a plugin providing it could
/// still be added to a field, i.e. fits next to the plugins there are, and what it requires in turn
/// could arrive as well.
///
/// The providers are judged by their static declarations, and a provider is trusted with what a
/// plugin it holds could provide, which is not looked into.
struct Reach<'a> {
    context: &'a Declaration,
    /// The fields a plugin could still be added to, each with its axis.
    open: &'a [(Path, AnyAxis)],
    /// The answered fields of the composite taking one plugin, each with its axis.
    answered: &'a [(Path, AnyAxis)],
    /// The plugins in the answers, to name the one keeping a provider out.
    placed: &'a [&'a Placed],
}

impl Reach<'_> {
    /// The fields `fact` could still arrive in next to plugins with `declared`: [`None`] if no
    /// field could take a plugin providing it at all, otherwise those fields whose providers fit,
    /// or, if none fits, the field whose plugin keeps them out, if a plugin does.
    fn fields(
        &self,
        declared: &[&Declared],
        fact: TypeId,
    ) -> Option<Result<Vec<Path>, Option<Path>>> {
        let mut able = self
            .open
            .iter()
            .filter(|(_, axis)| axis.facts().contains(fact))
            .peekable();
        able.peek()?;
        let mut fields = Vec::new();
        let mut blocker = None;
        for (path, axis) in able {
            match self.provided_by(*axis, declared, fact, &mut vec![fact]) {
                Ok(()) => fields.push(path.clone()),
                Err(blocked) => blocker = blocker.or(blocked),
            }
        }
        Some(if fields.is_empty() {
            Err(blocker)
        } else {
            Ok(fields)
        })
    }

    /// Whether a plugin of `axis` providing `fact` fits next to plugins with `declared`, see
    /// [`Self::fits`]; otherwise the field whose plugin keeps them out, if a plugin does. `visited`
    /// holds the facts looked for already, so that requirements in a cycle end.
    fn provided_by(
        &self,
        axis: AnyAxis,
        declared: &[&Declared],
        fact: TypeId,
        visited: &mut Vec<TypeId>,
    ) -> Result<(), Option<Path>> {
        let mut providers = axis
            .entries()
            .filter(|entry| entry.declaration().provides_type(fact))
            .peekable();
        // provided only by a plugin a plugin of the axis holds
        if providers.peek().is_none() {
            return Ok(());
        }
        let mut blocker = None;
        for entry in providers {
            match self.fits(entry, declared, visited) {
                Ok(()) => return Ok(()),
                Err(blocked) => blocker = blocker.or(blocked),
            }
        }
        Err(blocker)
    }

    /// Whether the plugin `entry` fits next to plugins with `declared`, with every fact it
    /// requires that nothing provides yet able to arrive in turn; otherwise the field whose plugin
    /// keeps it out, if a plugin does.
    fn fits(
        &self,
        entry: &PluginEntry,
        declared: &[&Declared],
        visited: &mut Vec<TypeId>,
    ) -> Result<(), Option<Path>> {
        let provider = Declared::new(entry.label(), entry.declaration());
        let own: OpenFacts = entry
            .sub_axes()
            .iter()
            .flat_map(|axis| axis.facts().iter())
            .collect();
        let with_provider: Vec<_> = declared.iter().copied().chain([&provider]).collect();
        for conflict in
            compatibility::conflicts(self.context, declared.iter().copied(), &provider, &own)
        {
            let fact = conflict.fact;
            let Incompatibility::FactNotProvided { .. } = conflict.incompatibility else {
                return Err(self.provider_of(fact));
            };
            if visited.contains(&fact) {
                return Err(None);
            }
            visited.push(fact);
            let mut able = self
                .open
                .iter()
                .filter(|(_, axis)| axis.facts().contains(fact))
                .peekable();
            let reached = if able.peek().is_none() {
                // only a field answered already could provide it
                Err(self
                    .answered
                    .iter()
                    .find(|(_, axis)| axis.facts().contains(fact))
                    .map(|(path, _)| path.clone()))
            } else {
                let mut reached = Err(None);
                for (_, axis) in able {
                    match self.provided_by(*axis, &with_provider, fact, visited) {
                        Ok(()) => {
                            reached = Ok(());
                            break;
                        }
                        Err(blocked) => {
                            if let Err(blocker) = &mut reached {
                                *blocker = blocker.take().or(blocked);
                            }
                        }
                    }
                }
                reached
            };
            visited.pop();
            reached?;
        }
        Ok(())
    }

    /// The path of the plugin in the answers that provides `fact`, if one does.
    fn provider_of(&self, fact: TypeId) -> Option<Path> {
        self.placed
            .iter()
            .find(|placed| placed.declared.declaration.provides_type(fact))
            .map(|placed| placed.path.clone())
    }
}

/// [`issues`] of the answers `tree` holds in a composite with `context`, with `lists` the lists of
/// the composite [`finish`] adds to. Without them it is finished: nothing is open or added any
/// more, and an unanswered field is an issue and provides nothing.
fn issues_in(
    context: &Declaration,
    tree: &Tree,
    lists: Option<&[(&'static str, AnyAxis)]>,
) -> Vec<Issue> {
    let finished = lists.is_none();
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

    // unless finished, a fact a field still unanswered could take a fitting provider for may still
    // arrive
    let unanswered: Vec<_> = if finished {
        Vec::new()
    } else {
        tree.questions
            .iter()
            .filter(|question| question.answer.is_none())
            .filter_map(|question| Some((question.path.clone(), slot(question.field.kind())?.0)))
            .collect()
    };
    let placed: Vec<_> = tree.plugins.iter().collect();
    let declarations = tree.declarations(None);
    let reach = Reach {
        context,
        open: &unanswered,
        answered: &[],
        placed: &placed,
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
        for conflict in
            compatibility::conflicts(context, others, &placed.declared, &OpenFacts::NOTHING)
        {
            match conflict.incompatibility {
                Incompatibility::FactNotProvided { .. }
                    if matches!(reach.fields(&declarations, conflict.fact), Some(Ok(_))) => {}
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
        let added_by_finish = lists.map_or_else(Vec::new, |lists| {
            additions(context, &declarations, lists, fact)
        });
        // a field still unanswered is found here only if none of its providers fits
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
                    added_by_finish,
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
                    added_by_finish: added_by_finish.clone(),
                },
            });
        }
    }
    issues
}

/// Settles the first fact the plugins in `answers`, the answers to `schema`, require that neither
/// they nor `context` provide and that is not in `tried`, see [`finish`]. Each fact looked at is
/// added to `tried`, so that it is tried once.
///
/// Returns whether plugins were added.
fn settle(
    context: &Declaration,
    schema: &Schema,
    answers: &mut Config,
    tried: &mut Vec<TypeId>,
) -> bool {
    let lists = top_lists(schema);
    let tree = Tree::of(schema, answers);
    let declarations = tree.declarations(None);
    for fact in compatibility::unprovided(&declarations) {
        if context.provides_type(fact) || tried.contains(&fact) {
            continue;
        }
        tried.push(fact);
        let Some(chain) = chain(context, &declarations, &lists, fact) else {
            continue;
        };
        for (name, entry) in chain {
            let mut list = match answers.get(name) {
                Some(Value::Array(list)) => list.clone(),
                _ => Vec::new(),
            };
            list.push(PluginConfig::new(entry.id()).into());
            answers.set(name, list);
        }
        return true;
    }
    false
}

/// The plugins [`finish`] adds to `lists` so that `fact` is provided in a composite with `context`
/// and plugins with `declarations`, each with the name of the list it is appended to, in the
/// order they are added in; [`None`] if it adds nothing.
///
/// The fact is settled by its [`settling`] plugin, and so is every fact that plugin requires that
/// nothing provides, in turn. The plugins are added only all together, and only if none of them
/// conflicts with the others or the context, none of them misses a fact, and none of them breaks
/// a condition an existing plugin requires of what it provides.
fn chain(
    context: &Declaration,
    declarations: &[&Declared],
    lists: &[(&'static str, AnyAxis)],
    fact: TypeId,
) -> Option<Vec<(&'static str, &'static PluginEntry)>> {
    let mut added: Vec<(&'static str, &'static PluginEntry, Declared)> = Vec::new();
    let mut pending = vec![fact];
    // every plugin added provides the fact it is added for, so each fact is settled at most once
    while let Some(fact) = pending.pop() {
        let provided = context.provides_type(fact)
            || declarations
                .iter()
                .copied()
                .chain(added.iter().map(|(_, _, declared)| declared))
                .any(|declared| declared.declaration.provides_type(fact));
        if provided {
            continue;
        }
        let (name, entry) = settling(lists, fact)?;
        let declaration = entry
            .declaration_for(&Config::new())
            .unwrap_or_else(|_| entry.declaration());
        let declared = Declared::new(entry.label(), declaration);
        pending.extend(
            declared
                .declaration
                .required()
                .iter()
                .map(RequiredFact::type_id),
        );
        added.push((name, entry, declared));
    }

    let all: Vec<_> = declarations
        .iter()
        .copied()
        .chain(added.iter().map(|(_, _, declared)| declared))
        .collect();
    let is_added_fact = |fact| {
        added
            .iter()
            .any(|(_, _, declared)| declared.declaration.provides_type(fact))
    };
    for (index, declared) in all.iter().enumerate() {
        let is_added = index >= declarations.len();
        let others = all
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != index)
            .map(|(_, other)| *other);
        // an existing plugin's own problems are not the chain's, unless they are about what the
        // chain provides
        if compatibility::conflicts(context, others, declared, &OpenFacts::NOTHING)
            .iter()
            .any(|conflict| is_added || is_added_fact(conflict.fact))
        {
            return None;
        }
    }
    Some(
        added
            .into_iter()
            .map(|(name, entry, _)| (name, entry))
            .collect(),
    )
}

/// The fields of `schema` taking a list of plugins, by name, each with its axis.
fn top_lists(schema: &Schema) -> Vec<(&'static str, AnyAxis)> {
    schema
        .fields()
        .iter()
        .filter_map(|field| match slot(field.kind()) {
            Some((axis, true)) => Some((field.name(), axis)),
            _ => None,
        })
        .collect()
}

/// The plugin [`finish`] settles `fact` with, and the name of the list among `lists` it is added
/// to: the only plugin without fields providing it among the axes of `lists`, or [`None`] if
/// there is none or more than one.
fn settling(
    lists: &[(&'static str, AnyAxis)],
    fact: TypeId,
) -> Option<(&'static str, &'static PluginEntry)> {
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
    let settled = providers.next()?;
    providers.next().is_none().then_some(settled)
}

/// What [`finish`] adds for `fact`, see [`chain`]; empty if it adds nothing.
fn additions(
    context: &Declaration,
    declarations: &[&Declared],
    lists: &[(&'static str, AnyAxis)],
    fact: TypeId,
) -> Vec<Addition> {
    chain(context, declarations, lists, fact)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, entry)| Addition {
            list: Path::ROOT.field(name),
            plugin: entry.id(),
        })
        .collect()
}

/// [`default_value`] of `field`, the field at `path` in the answers `tree` holds, in a composite
/// with `context`.
fn default_in(context: &Declaration, tree: &Tree, field: &Field, path: &Path) -> Option<Value> {
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
    let mut offered = Vec::new();
    let mut fitting = Vec::new();
    for (entry, recommended) in recommended {
        // a list takes each plugin once, so a plugin recommended by several siblings is offered
        // where it is first recommended and fits
        if offered.contains(&entry.id()) {
            continue;
        }
        // a recommendation is a whole answer, so what it declares includes the plugins answering
        // its fields
        let declaration = entry
            .declaration_for(recommended.config())
            .unwrap_or_else(|_| entry.declaration());
        let declared = Declared::new(entry.label(), declaration);
        if compatibility::check_addition(
            context,
            declarations.iter().copied().chain(&added),
            &declared,
            &open,
        )
        .is_ok()
        {
            added.push(declared);
            offered.push(entry.id());
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

/// A plugin that could answer a position taking plugins, with how it fits, see [`candidates`].
///
/// It fits if it has no [conflicts](Self::conflicts), and fits as it is if nothing is
/// [missing](Self::missing) either.
#[derive(Debug)]
pub struct Candidate {
    entry: &'static PluginEntry,
    conflicts: Vec<Conflict>,
    missing: Vec<Missing>,
    supplies: Vec<Supply>,
}

impl Candidate {
    /// The plugin.
    pub fn entry(&self) -> &'static PluginEntry {
        self.entry
    }

    /// Why the plugin does not fit, if it does not: the first fact it provides that is provided
    /// already, then every requirement no plugin still to be added can meet, in the order it
    /// requires them in.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// What the plugin requires that is still to be added, in the order it requires it in.
    pub fn missing(&self) -> &[Missing] {
        &self.missing
    }

    /// What the plugin provides that the other plugins require and nothing provides yet.
    pub fn supplies(&self) -> &[Supply] {
        &self.supplies
    }
}

/// Why a [`Candidate`] does not fit.
#[derive(Debug, PartialEq, Eq)]
pub struct Conflict {
    incompatibility: Incompatibility,
    with: Option<Path>,
}

impl Conflict {
    /// What does not fit.
    pub fn incompatibility(&self) -> &Incompatibility {
        &self.incompatibility
    }

    /// The field or position in a list whose plugin has to change for the candidate to fit, or
    /// [`None`] if no change makes it fit: the fact is the context's or the candidate's own.
    pub fn with(&self) -> Option<&Path> {
        self.with.as_ref()
    }
}

/// A fact a [`Candidate`] requires that nothing provides yet, but a plugin still to be added
/// could.
#[derive(Debug, PartialEq, Eq)]
pub struct Missing {
    fact: &'static str,
    fields: Vec<Path>,
    added_by_finish: Vec<Addition>,
}

impl Missing {
    /// What the fact is called.
    pub fn fact(&self) -> &'static str {
        self.fact
    }

    /// The fields a plugin providing the fact could be added to: fields still unanswered and
    /// lists, in the order of the tree.
    pub fn fields(&self) -> &[Path] {
        &self.fields
    }

    /// What [`finish`] adds, in this order, for the fact if it is still missing then: a plugin
    /// providing it and the plugins providing what that one requires in turn. Empty if it adds
    /// nothing and a plugin has to be chosen.
    pub fn added_by_finish(&self) -> &[Addition] {
        &self.added_by_finish
    }
}

/// A plugin [`finish`] adds to provide a fact nothing provides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addition {
    list: Path,
    plugin: &'static str,
}

impl Addition {
    /// The list of the composite the plugin is appended to.
    pub fn list(&self) -> &Path {
        &self.list
    }

    /// The id of the plugin.
    pub fn plugin(&self) -> &'static str {
        self.plugin
    }
}

/// A fact a [`Candidate`] provides that other plugins require and nothing provides yet.
#[derive(Debug, PartialEq, Eq)]
pub struct Supply {
    fact: &'static str,
    required_by: Vec<Path>,
    other_providers: Vec<&'static str>,
}

impl Supply {
    /// What the fact is called by the first plugin requiring it.
    pub fn fact(&self) -> &'static str {
        self.fact
    }

    /// The plugins requiring the fact, in the order of the tree.
    pub fn required_by(&self) -> &[Path] {
        &self.required_by
    }

    /// The ids of the other plugins providing the fact among the axes reachable from the
    /// composite, each once; empty if the candidate is the only one.
    pub fn other_providers(&self) -> &[&'static str] {
        &self.other_providers
    }
}

/// Something currently wrong with the answers of a [`Draft`], at the path where it is fixed, see
/// [`issues`].
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

/// Shows the path followed by the problem, as `path: problem`.
impl Display for Issue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.problem)
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
    /// could take a fitting provider for. The issue is at each field whose axis could provide it,
    /// with the ids of the plugins of that axis that do, or at `required_by` if there is none.
    ///
    /// `added_by_finish` is what [`finish`] adds for the fact, in this order, so that the issue
    /// fixes itself without raising another: a plugin providing it and the plugins providing what
    /// that one requires in turn. Empty if it adds nothing, and always in the issues `finish`
    /// returns.
    Unprovided {
        fact: &'static str,
        required_by: Path,
        providers: Vec<&'static str>,
        added_by_finish: Vec<Addition>,
    },
}

/// Shows an [`Unprovided`](Self::Unprovided) like [`Incompatibility::FactNotProvided`], followed
/// by the providers in parentheses if there are any.
impl Display for Problem {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(error) => error.fmt(formatter),
            Self::Incompatible(incompatibility) => incompatibility.fmt(formatter),
            Self::Unprovided {
                fact,
                required_by,
                providers,
                ..
            } => {
                let unprovided = Incompatibility::FactNotProvided {
                    fact,
                    required_by: required_by.to_string(),
                };
                write!(formatter, "{unprovided}")?;
                if !providers.is_empty() {
                    write!(formatter, " (provided by {})", providers.join(", "))?;
                }
                Ok(())
            }
        }
    }
}

/// The axis of the plugins a position of `kind` takes and whether it takes a list of them, or
/// [`None`] if it takes no plugin.
fn slot(kind: &FieldKind) -> Option<(AnyAxis, bool)> {
    match unwrapped(kind) {
        FieldKind::Plugin(axis) => Some((*axis, false)),
        FieldKind::List { element, .. } => match unwrapped(element) {
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
                let (FieldKind::List { element, .. }, Some(Value::Array(answers))) =
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

/// [`Draft::set`] on `answers`, the answers to `schema`.
fn set_in(
    schema: &Schema,
    answers: &mut Config,
    path: &Path,
    answer: &Value,
) -> Result<(), ConfigError> {
    let position = locate(schema, answers, path)?;
    let answer = position.kind.walk(answer, path, false)?;
    let mut edited = answers.clone();
    edit(&mut edited, path, Some(answer));
    // a plugin put into a list repeats one of the list only next to the rest of it; the other
    // answers were checked when they were set, so this finds nothing else
    schema.walk(&edited, &Path::ROOT, false)?;
    *answers = edited;
    Ok(())
}

/// Answers the position at `path` in `answers` with `answer`, or takes its answer back if
/// `answer` is [`None`], see [`Draft::set`] and [`Draft::unset`]. `path` has been located.
fn edit(answers: &mut Config, path: &Path, answer: Option<Value>) {
    let (last, parent) = path
        .segments()
        .split_last()
        .expect("a located path is not the root");
    match last {
        Segment::Field(name) => {
            // the answers of a plugin are its answer's "value", which checking an answer writes
            // as an object
            let config = if parent.is_empty() {
                &mut answers.0
            } else {
                answers
                    .get_at_mut(parent)
                    .and_then(|plugin| plugin.get_mut("value"))
                    .and_then(Value::as_object_mut)
                    .expect("a located field is in the config of a plugin")
            };
            match answer {
                Some(answer) => config.insert(name.clone(), answer),
                None => config.remove(name),
            };
        }
        Segment::Index(index) => {
            let list = answers
                .get_at_mut(parent)
                .and_then(Value::as_array_mut)
                .expect("a located index is in a list");
            match answer {
                Some(answer) if *index == list.len() => list.push(answer),
                Some(answer) => list[*index] = answer,
                None => {
                    list.remove(*index);
                }
            }
        }
    }
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
    /// The plugin's id, see [`PluginType::ID`](crate::PluginType::ID).
    id: &'static str,
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
            (FieldKind::List { element, .. }, Value::Array(answers)) => {
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
            id: entry.id(),
            path,
            parent: parent.clone(),
            instance,
            declared,
        }
    }
}

/// The draft, tested on axes of its own so that it does not depend on what the plugins of a
/// program happen to register.
#[cfg(test)]
mod tests {
    use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
    use serde_json::json;

    use super::*;
    use crate::{Plugin, PluginType, axis, compatibility::Fact, register};

    #[derive(Debug)]
    struct Points;

    impl Fact for Points {}

    #[derive(Debug)]
    struct Matches;

    impl Fact for Matches {}

    #[derive(Debug)]
    struct Eliminating;

    impl Fact for Eliminating {}

    /// How many entrants a ranking stands for, the fact the context of the composite provides.
    #[derive(Debug)]
    struct EntrantsPerRanking(u32);

    impl Fact for EntrantsPerRanking {}

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
        description = "a plugin of the draft's tests",
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
        description = "a plugin of the draft's tests",
        declare_static = Self::declare_static
    )]
    struct Knockout {
        #[plugin(label = "Third place", default)]
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
        description = "a plugin of the draft's tests",
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

    /// A format with a condition on the fact the context provides, recommending a metric twice.
    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "monster",
        label = "monster",
        description = "a plugin of the draft's tests",
        declare_static = Self::declare_static,
        recommends = Self::recommends
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

        fn recommends(&self) -> Vec<PluginConfig> {
            vec![
                PluginConfig::of::<Buchholz>(),
                PluginConfig::of::<MatchCount>(),
                PluginConfig::of::<Buchholz>(),
            ]
        }
    }

    #[register]
    impl Format for Monster {}

    #[derive(Debug, Serialize, Deserialize, Plugin)]
    #[plugin(
        id = "match_count",
        label = "match_count",
        description = "a plugin of the draft's tests",
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
        description = "a plugin of the draft's tests",
        declare_static = Self::declare_static
    )]
    struct Wins {
        #[serde(deserialize_with = "non_negative")]
        #[plugin(label = "Points for a win", default = 1.0)]
        for_win: f64,
    }

    impl Wins {
        fn declare_static(declaration: &mut Declaration) {
            declaration.provide(Points);
        }
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
        description = "a plugin of the draft's tests",
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
        description = "a plugin of the draft's tests",
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
        description = "a plugin of the draft's tests",
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
        #[plugin(label = "Parts", default)]
        parts: Vec<Box<dyn Metric>>,
    }

    #[register]
    impl Metric for Combined {}

    /// The composite: one format, any number of metrics, and a metric or none.
    #[derive(Debug, Serialize, Deserialize, Configurable)]
    struct Phase {
        #[plugin(label = "Format")]
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

    /// A draft of a phase of single players.
    fn draft() -> Draft<Phase> {
        Draft::new()
    }

    /// The path written as `text`, e.g. `metrics[1].for_win`.
    fn path(text: &str) -> Path {
        text.parse().unwrap()
    }

    /// The plugin `P` with nothing answered.
    fn plugin<P: PluginType>() -> PluginConfig {
        PluginConfig::of::<P>()
    }

    /// A draft with `answers` set, each at the path written as its first element.
    fn answered(answers: &[(&str, Value)]) -> Draft<Phase> {
        let mut draft = draft();
        for (at, answer) in answers {
            draft.set(&path(at), answer.clone()).unwrap();
        }
        draft
    }

    /// The candidates at `at` in a phase of single players, by id, each with why it does not fit.
    fn candidates(draft: &Draft<Phase>, at: &str) -> Vec<(&'static str, Vec<Conflict>)> {
        super::candidates(singles, draft, &path(at))
            .unwrap()
            .into_iter()
            .map(|candidate| (candidate.entry().id(), candidate.conflicts))
            .collect()
    }

    /// The plugin `id` as a candidate at `at` in a phase of single players.
    fn candidate_at(draft: &Draft<Phase>, at: &str, id: &str) -> Candidate {
        super::candidates(singles, draft, &path(at))
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.entry().id() == id)
            .expect("the plugin is a candidate")
    }

    /// Why the plugin `id` does not fit at `at`.
    fn conflicts_of(draft: &Draft<Phase>, at: &str, id: &str) -> Vec<Conflict> {
        candidate_at(draft, at, id).conflicts
    }

    /// A conflict with the plugin at `with`, or with no plugin.
    fn conflict(incompatibility: Incompatibility, with: Option<&str>) -> Conflict {
        Conflict {
            incompatibility,
            with: with.map(path),
        }
    }

    /// The plugin `plugin` appended to the list `list` by `finish`.
    fn addition(list: &str, plugin: &'static str) -> Addition {
        Addition {
            list: path(list),
            plugin,
        }
    }

    /// The fact `fact` missing, to be added to `fields`, by `finish` as `added` if it adds it.
    fn missing(fact: &'static str, fields: &[&str], added: &[Addition]) -> Missing {
        Missing {
            fact,
            fields: fields.iter().copied().map(path).collect(),
            added_by_finish: added.to_vec(),
        }
    }

    /// The paths of the fields in the tree of the answers of `draft`, each with whether it is
    /// answered.
    fn paths<T: Configurable>(draft: &Draft<T>) -> Vec<(String, bool)> {
        Tree::of(&T::schema(), draft.answers())
            .questions
            .iter()
            .map(|question| (question.path.to_string(), question.answer.is_some()))
            .collect()
    }

    #[test]
    fn the_tree_holds_the_fields_of_the_composite_and_of_the_plugins_answering_them() {
        let mut draft = draft();

        assert_eq!(
            paths(&draft),
            [
                ("format".to_owned(), false),
                ("metrics".to_owned(), false),
                ("tie_break".to_owned(), false),
            ]
        );
        draft.set(&path("format"), plugin::<Knockout>()).unwrap();
        assert_eq!(
            paths(&draft),
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
        let mut draft = answered(&[("metrics", json!([plugin::<Wins>()]))]);

        draft.set(&path("metrics[0].for_win"), 2).unwrap();
        draft
            .set(&path("metrics[1]"), plugin::<MatchCount>())
            .unwrap();
        assert_eq!(
            draft.answers().get("metrics"),
            Some(&json!([
                {"type": "wins", "value": {"for_win": 2}},
                {"type": "match_count", "value": {}},
            ]))
        );

        draft.unset(&path("metrics[0]")).unwrap();
        assert_eq!(
            draft.answers().get("metrics"),
            Some(&json!([{"type": "match_count", "value": {}}]))
        );
        draft.unset(&path("metrics")).unwrap();
        assert_eq!(draft.answers().get("metrics"), None);
    }

    #[test]
    fn a_path_addressing_nothing_is_an_error() {
        let mut draft = answered(&[("metrics", json!([]))]);

        for at in ["format.third_place", "metrics[1]", "rounds", "tie_break[0]"] {
            assert_eq!(
                draft.set(&path(at), 1),
                Err(ConfigError::UnknownField { path: path(at) }),
                "{at}"
            );
        }
        assert_eq!(
            draft.unset(&path("metrics[0]")),
            Err(ConfigError::UnknownField {
                path: path("metrics[0]"),
            })
        );
    }

    #[test]
    fn an_answer_that_is_not_one_its_field_takes_is_not_set() {
        let mut draft = draft();

        assert_eq!(
            draft.set(&path("metrics"), 1),
            Err(ConfigError::WrongKind {
                path: path("metrics"),
                expected: "a list",
                found: "a whole number",
            })
        );
        assert_eq!(
            draft.set(
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
            draft.set(&path("format"), plugin::<Wins>()),
            Err(ConfigError::UnknownPlugin {
                path: path("format"),
            })
        );
        assert_eq!(draft.answers(), &Config::new());
    }

    #[test]
    fn a_plugin_already_in_a_list_is_not_set_into_it_again() {
        let mut draft = answered(&[("metrics", json!([plugin::<Wins>(), plugin::<Buchholz>()]))]);
        let before = draft.answers().clone();
        let duplicate = |at: &str| Err(ConfigError::Duplicate { path: path(at) });
        let wins = || plugin::<Wins>().with("for_win", 2);

        // whatever it is configured with, appended or in another position
        assert_eq!(
            draft.set(&path("metrics[2]"), wins()),
            duplicate("metrics[2]")
        );
        assert_eq!(
            draft.set(&path("metrics[1]"), wins()),
            duplicate("metrics[1]")
        );
        assert_eq!(
            draft.set(&path("metrics"), json!([wins(), wins()])),
            duplicate("metrics[1]")
        );
        assert_eq!(draft.answers(), &before);

        // a plugin replacing itself is no repeat
        draft.set(&path("metrics[0]"), wins()).unwrap();
        assert_eq!(
            draft.answers().get("metrics"),
            Some(&json!([wins(), plugin::<Buchholz>()]))
        );
    }

    #[test]
    fn a_candidate_conflicting_with_the_context_says_why() {
        assert_eq!(
            candidates(&draft(), "format"),
            [
                (
                    "doubles",
                    vec![conflict(
                        Incompatibility::FactProvidedByContext {
                            fact: format!("{:?}", EntrantsPerRanking(2)),
                            provider: "doubles".to_owned(),
                        },
                        None
                    )]
                ),
                ("knockout", vec![]),
                ("monster", vec![]),
                ("round_robin", vec![]),
            ]
        );
    }

    #[test]
    fn a_candidate_does_not_conflict_with_what_it_replaces() {
        let draft = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Wins>()])),
            ("tie_break", Value::Null),
        ]);

        assert_eq!(conflicts_of(&draft, "metrics[0]", "score"), []);
        assert_eq!(conflicts_of(&draft, "metrics", "score"), []);
        assert_eq!(
            conflicts_of(&draft, "metrics[1]", "score"),
            [conflict(
                Incompatibility::FactProvidedTwice {
                    fact: format!("{Points:?}"),
                    first: "wins (metrics[0])".to_owned(),
                    second: "score".to_owned(),
                },
                Some("metrics[0]")
            )]
        );
    }

    #[test]
    fn a_plugin_answering_another_position_of_a_list_is_no_candidate_there() {
        let draft = answered(&[
            ("metrics", json!([plugin::<Buchholz>(), plugin::<Wins>()])),
            (
                "tie_break",
                plugin::<Combined>().with("parts", json!([])).into(),
            ),
        ]);
        let ids = |at| -> Vec<_> {
            candidates(&draft, at)
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        };
        let all = [
            "buchholz",
            "combined",
            "losses",
            "match_count",
            "score",
            "wins",
        ];

        // the wins are left out whatever they are configured with
        assert_eq!(
            ids("metrics[2]"),
            ["combined", "losses", "match_count", "score"]
        );
        // what answers the position itself stays a candidate, and so does what answers another
        // field
        assert_eq!(
            ids("metrics[0]"),
            ["buchholz", "combined", "losses", "match_count", "score"]
        );
        assert_eq!(ids("metrics"), all);
        assert_eq!(ids("tie_break.parts[0]"), all);
    }

    #[test]
    fn a_candidate_requiring_what_an_unanswered_field_could_provide_misses_it() {
        let draft = answered(&[("tie_break", Value::Null)]);
        let losses = candidate_at(&draft, "metrics", "losses");

        assert_eq!(losses.conflicts, []);
        assert_eq!(losses.missing, [missing("eliminations", &["format"], &[])]);
    }

    #[test]
    fn a_candidate_requiring_what_only_an_answered_single_field_could_provide_conflicts_with_it() {
        let draft = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("tie_break", Value::Null),
        ]);
        let losses = candidate_at(&draft, "metrics", "losses");

        assert_eq!(
            losses.conflicts,
            [conflict(
                Incompatibility::FactNotProvided {
                    fact: "eliminations",
                    required_by: "losses".to_owned(),
                },
                Some("format")
            )]
        );
        assert_eq!(losses.missing, []);
    }

    #[test]
    fn a_candidate_requiring_what_a_list_could_provide_misses_it() {
        let draft = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Wins>()])),
            ("tie_break", Value::Null),
        ]);
        let expected = [missing(
            "matches",
            &["metrics"],
            &[addition("metrics", "match_count")],
        )];

        // in the list, and anywhere else
        for at in ["metrics[1]", "tie_break"] {
            let buchholz = candidate_at(&draft, at, "buchholz");
            assert_eq!(buchholz.conflicts, []);
            assert_eq!(buchholz.missing, expected);
        }
    }

    #[test]
    fn a_missing_fact_finish_cannot_settle_is_to_be_chosen() {
        let draft = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("tie_break", Value::Null),
        ]);

        // only score provides points without being asked anything, wins has a field
        assert_eq!(
            candidate_at(&draft, "metrics", "buchholz").missing,
            [
                missing("points", &["metrics"], &[addition("metrics", "score")]),
                missing(
                    "matches",
                    &["metrics"],
                    &[addition("metrics", "match_count")]
                ),
            ]
        );
        assert_eq!(
            candidate_at(&answered(&[]), "metrics", "losses").missing,
            [missing("eliminations", &["format"], &[])]
        );
    }

    #[test]
    fn a_candidate_supplies_what_the_others_require_and_nothing_provides() {
        let draft = answered(&[("metrics", json!([plugin::<Buchholz>(), plugin::<Losses>()]))]);

        assert_eq!(
            candidate_at(&draft, "format", "knockout").supplies,
            [Supply {
                fact: "eliminations",
                required_by: vec![path("metrics[1]")],
                other_providers: vec![],
            }]
        );
        assert_eq!(
            candidate_at(&draft, "tie_break", "score").supplies,
            [Supply {
                fact: "points",
                required_by: vec![path("metrics[0]")],
                other_providers: vec!["wins"],
            }]
        );
        assert_eq!(candidate_at(&draft, "format", "round_robin").supplies, []);
    }

    #[test]
    fn a_position_taking_no_plugins_has_no_candidates() {
        let draft = answered(&[("format", plugin::<Knockout>().into())]);

        assert!(candidates(&draft, "format.third_place").is_empty());
    }

    #[test]
    fn nested_plugins_are_addressed_through_the_fields_they_answer() {
        let mut draft = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Combined>()])),
        ]);

        assert!(paths(&draft).contains(&("metrics[0].parts".to_owned(), false)));
        draft
            .set(&path("metrics[0].parts"), json!([plugin::<Wins>()]))
            .unwrap();
        assert_eq!(
            conflicts_of(&draft, "metrics[0].parts[1]", "score"),
            [conflict(
                Incompatibility::FactProvidedTwice {
                    fact: format!("{Points:?}"),
                    first: "wins (metrics[0].parts[0])".to_owned(),
                    second: "score".to_owned(),
                },
                Some("metrics[0].parts[0]")
            )]
        );
    }

    #[test]
    fn an_answer_a_plugin_rejects_is_an_issue_at_its_field() {
        let draft = answered(&[("metrics", json!([plugin::<Wins>().with("for_win", -1.0)]))]);
        let rejected = ConfigError::Rejected {
            path: path("metrics[0].for_win"),
            reason: "the points for a win cannot be negative".to_owned(),
        };

        assert_eq!(
            issues(singles, &draft),
            [Issue {
                path: path("metrics[0].for_win"),
                problem: Problem::Invalid(rejected),
            }]
        );
    }

    #[test]
    fn plugins_providing_the_same_fact_are_issues_at_each_of_them() {
        let draft = answered(&[
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
            issues(singles, &draft),
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
        let unprovided = |fact, providers: &[&'static str], added_by_finish| Problem::Unprovided {
            fact,
            required_by: path("metrics[0]"),
            providers: providers.to_vec(),
            added_by_finish,
        };
        // finishing appends the match count, which provides the matches without being asked
        let counted = vec![addition("metrics", "match_count")];

        assert_eq!(
            issues(singles, &losing),
            [Issue {
                path: path("format"),
                problem: unprovided("eliminations", &["knockout"], vec![]),
            }]
        );
        assert_eq!(
            issues(singles, &counting),
            [
                Issue {
                    path: path("metrics"),
                    problem: unprovided("matches", &["match_count"], counted.clone()),
                },
                Issue {
                    path: path("tie_break"),
                    problem: unprovided("matches", &["match_count"], counted),
                },
            ]
        );
    }

    #[test]
    fn a_fact_an_unanswered_field_could_provide_is_no_issue() {
        let draft = answered(&[("metrics", json!([plugin::<Losses>()]))]);

        assert_eq!(issues(singles, &draft), []);
    }

    #[test]
    fn a_field_offers_the_default_of_its_schema() {
        let draft = answered(&[("format", plugin::<Knockout>().into())]);

        assert_eq!(
            default_value(singles, &draft, &path("format.third_place")),
            Ok(Some(json!(false)))
        );
        assert_eq!(
            default_value(singles, &draft, &path("metrics")),
            Ok(Some(json!([])))
        );
        assert_eq!(default_value(singles, &draft, &path("format")), Ok(None));
    }

    #[test]
    fn a_field_taking_a_list_offers_every_recommendation_that_fits() {
        let draft = answered(&[("format", plugin::<RoundRobin>().into())]);

        assert_eq!(
            default_value(singles, &draft, &path("metrics")),
            Ok(Some(json!([plugin::<MatchCount>(), plugin::<Wins>()])))
        );
    }

    #[test]
    fn a_field_taking_a_list_offers_a_recommendation_once() {
        let draft = answered(&[("format", plugin::<Monster>().into())]);

        assert_eq!(
            default_value(singles, &draft, &path("metrics")),
            Ok(Some(json!([plugin::<Buchholz>(), plugin::<MatchCount>()])))
        );
    }

    #[test]
    fn a_field_taking_one_plugin_offers_the_first_recommendation_that_fits() {
        let mut draft = answered(&[("format", plugin::<RoundRobin>().into())]);

        assert_eq!(
            default_value(singles, &draft, &path("tie_break")),
            Ok(Some(plugin::<MatchCount>().into()))
        );
        draft
            .set(&path("metrics"), json!([plugin::<MatchCount>()]))
            .unwrap();
        assert_eq!(
            default_value(singles, &draft, &path("tie_break")),
            Ok(Some(plugin::<Wins>().into()))
        );
    }

    /// Answers every unanswered field of `draft` that offers an answer with it, depth first in
    /// schema order, as a UI accepting every default does.
    fn accept_defaults(draft: &mut Draft<Phase>) {
        while let Some(at) = Tree::of(&Phase::schema(), draft.answers())
            .questions
            .into_iter()
            .find(|question| {
                question.answer.is_none()
                    && default_value(singles, draft, &question.path) != Ok(None)
            })
            .map(|question| question.path)
        {
            let default = default_value(singles, draft, &at).unwrap().unwrap();
            draft.set(&at, default).unwrap();
        }
    }

    #[test]
    fn finishing_fills_in_no_default() {
        let mut draft = answered(&[("format", plugin::<RoundRobin>().into())]);
        let unanswered = |at: &str| Issue {
            path: path(at),
            problem: Problem::Invalid(ConfigError::Unanswered { path: path(at) }),
        };

        assert_eq!(
            finish(singles, &draft).err(),
            Some(vec![unanswered("metrics"), unanswered("tie_break")])
        );
        accept_defaults(&mut draft);
        assert_eq!(
            finish(singles, &draft).map(|phase| format!("{phase:?}")),
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
            finish(singles, &draft()).err(),
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

        assert!(finish(|_| {}, &Draft::<Nothing>::new()).is_ok());
    }

    #[test]
    fn finishing_with_an_answer_a_plugin_rejects_fails() {
        let draft = answered(&[
            ("format", plugin::<RoundRobin>().into()),
            ("metrics", json!([plugin::<Wins>().with("for_win", -1.0)])),
            ("tie_break", Value::Null),
        ]);

        assert_eq!(
            finish(singles, &draft).err(),
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
        let draft = answered(&[
            ("metrics", json!([plugin::<Losses>()])),
            ("tie_break", Value::Null),
        ]);

        // the unanswered format could still provide the eliminations
        assert_eq!(issues(singles, &draft), []);
        assert_eq!(
            finish(singles, &draft).err(),
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
                        added_by_finish: vec![],
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
            finish(singles, &settled).map(|phase| format!("{:?}", phase.metrics)),
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
        let draft = Draft::from_built(&phase).unwrap();

        assert_eq!(
            draft.answers().get("format"),
            Some(&json!({"type": "knockout", "value": {"third_place": true}}))
        );
        assert_eq!(
            draft.answers().get("metrics"),
            Some(&json!([plugin::<Buchholz>(), plugin::<Score>()]))
        );
        assert_eq!(
            finish(singles, &draft).map(|phase| format!("{phase:?}")),
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
    /// A fact nothing provides yet only counts as still to arrive while a plugin providing it
    /// could still be added, tested on axes of their own: a criterion requiring rounds of
    /// elimination, which only a calculator requiring a format that eliminates calculates.
    mod reachability {
        use super::*;

        #[derive(Debug)]
        struct EliminationRounds;

        impl Fact for EliminationRounds {}

        #[axis]
        trait Schedule: Plugin {}

        #[axis]
        trait Criterion: Plugin {}

        #[axis]
        trait Calculator: Plugin {}

        /// A format eliminating nobody.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(id = "everyone", label = "everyone")]
        struct Everyone;

        #[register]
        impl Schedule for Everyone {}

        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "eliminating",
            label = "eliminating",
            declare_static = Self::declare_static
        )]
        struct EliminatingSchedule;

        impl EliminatingSchedule {
            fn declare_static(declaration: &mut Declaration) {
                declaration.provide(Eliminating);
            }
        }

        #[register]
        impl Schedule for EliminatingSchedule {}

        /// The only calculator of the elimination rounds, which needs a format that eliminates.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "elimination_rounds",
            label = "elimination rounds",
            declare_static = Self::declare_static
        )]
        struct EliminationRoundsCalculator;

        impl EliminationRoundsCalculator {
            fn declare_static(declaration: &mut Declaration) {
                declaration
                    .provide(EliminationRounds)
                    .require::<Eliminating>("eliminations");
            }
        }

        #[register]
        impl Calculator for EliminationRoundsCalculator {}

        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "later_elimination",
            label = "later elimination",
            declare_static = Self::declare_static
        )]
        struct LaterElimination;

        impl LaterElimination {
            fn declare_static(declaration: &mut Declaration) {
                declaration.require::<EliminationRounds>("the elimination rounds");
            }
        }

        #[register]
        impl Criterion for LaterElimination {}

        #[derive(Debug, Serialize, Deserialize, Configurable)]
        struct Ranked {
            #[plugin(label = "format")]
            format: Box<dyn Schedule>,
            #[plugin(label = "ranked by")]
            ranking: Vec<Box<dyn Criterion>>,
            #[plugin(label = "metrics")]
            metrics: Vec<Box<dyn Calculator>>,
        }

        /// A draft with an empty ranking and the format `format`, if any.
        fn ranked(format: Option<PluginConfig>) -> Draft<Ranked> {
            let mut draft = Draft::new();
            draft.set(&path("ranking"), json!([])).unwrap();
            if let Some(format) = format {
                draft.set(&path("format"), format).unwrap();
            }
            draft
        }

        /// The criterion as the candidate for the first position of the ranking of `draft`.
        fn criterion(draft: &Draft<Ranked>) -> Candidate {
            let mut candidates =
                crate::draft::candidates(|_| {}, draft, &path("ranking[0]")).unwrap();
            assert_eq!(candidates.len(), 1);
            candidates.remove(0)
        }

        #[test]
        fn a_requirement_whose_only_provider_does_not_fit_is_a_conflict_with_what_keeps_it_out() {
            let candidate = criterion(&ranked(Some(plugin::<Everyone>())));

            assert!(candidate.missing().is_empty());
            assert_eq!(
                candidate
                    .conflicts()
                    .iter()
                    .map(|conflict| (conflict.incompatibility().clone(), conflict.with().cloned()))
                    .collect::<Vec<_>>(),
                [(
                    Incompatibility::FactNotProvided {
                        fact: "the elimination rounds",
                        required_by: "later elimination".to_owned(),
                    },
                    Some(path("format"))
                )]
            );
        }

        #[test]
        fn a_requirement_whose_provider_fits_is_missing() {
            let candidate = criterion(&ranked(Some(plugin::<EliminatingSchedule>())));

            assert!(candidate.conflicts().is_empty());
            assert_eq!(
                candidate
                    .missing()
                    .iter()
                    .map(|missing| missing.fields().to_vec())
                    .collect::<Vec<_>>(),
                [vec![path("metrics")]]
            );
        }

        /// What the provider requires may still arrive itself, as long as a field could take it.
        #[test]
        fn a_provider_whose_own_requirement_could_still_arrive_fits() {
            let candidate = criterion(&ranked(None));

            assert!(candidate.conflicts().is_empty());
            assert_eq!(candidate.missing().len(), 1);
        }

        #[test]
        fn an_unanswered_field_whose_providers_do_not_fit_leaves_nothing_open() {
            let unprovided = |format| {
                let mut draft = ranked(Some(format));
                draft
                    .set(&path("ranking"), json!([plugin::<LaterElimination>()]))
                    .unwrap();
                issues(|_| {}, &draft)
                    .into_iter()
                    .filter(|issue| matches!(issue.problem(), Problem::Unprovided { .. }))
                    .map(|issue| issue.path().clone())
                    .collect::<Vec<_>>()
            };

            assert_eq!(unprovided(plugin::<Everyone>()), [path("metrics")]);
            assert_eq!(unprovided(plugin::<EliminatingSchedule>()), []);
        }
    }

    mod resolution {
        use super::*;

        #[derive(Debug)]
        struct Settled;

        impl Fact for Settled {}

        #[derive(Debug)]
        struct Ambiguous;

        impl Fact for Ambiguous {}

        #[derive(Debug)]
        struct Downstream;

        impl Fact for Downstream {}

        #[derive(Debug)]
        struct Deep;

        impl Fact for Deep {}

        #[derive(Debug)]
        struct Crosswise;

        impl Fact for Crosswise {}

        #[derive(Debug)]
        struct Lone;

        impl Fact for Lone {}

        #[derive(Debug)]
        struct Nested;

        impl Fact for Nested {}

        #[derive(Debug)]
        struct Stranded;

        impl Fact for Stranded {}

        #[derive(Debug)]
        struct Clash;

        impl Fact for Clash {}

        /// A fact [`Clashing`] provides, so that it clashes with a context providing it too.
        #[derive(Debug)]
        struct Given;

        impl Fact for Given {}

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
            #[serde(default)]
            #[plugin(label = "Stranded")]
            stranded: bool,
            #[serde(default)]
            #[plugin(label = "Clash")]
            clash: bool,
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
                if self.stranded {
                    declaration.require::<Stranded>("the stranded fact");
                }
                if self.clash {
                    declaration.require::<Clash>("the clash fact");
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

        /// The only plugin providing [`Stranded`], requiring what is ambiguous.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "stranding",
            label = "stranding",
            description = "a plugin of the resolution tests",
            declare_static = Self::declare_static
        )]
        struct Stranding;

        impl Stranding {
            fn declare_static(declaration: &mut Declaration) {
                declaration
                    .provide(Stranded)
                    .require::<Ambiguous>("the ambiguous fact");
            }
        }

        #[register]
        impl Supply for Stranding {}

        /// The only plugin providing [`Clash`], providing [`Given`] as well.
        #[derive(Debug, Serialize, Deserialize, Plugin)]
        #[plugin(
            id = "clashing",
            label = "clashing",
            description = "a plugin of the resolution tests",
            declare_static = Self::declare_static
        )]
        struct Clashing;

        impl Clashing {
            fn declare_static(declaration: &mut Declaration) {
                declaration.provide(Clash).provide(Given);
            }
        }

        #[register]
        impl Supply for Clashing {}

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
            context: impl FnOnce(&mut Declaration),
            requires: &[&str],
            answers: &[(&str, Value)],
        ) -> Result<Vec<String>, Vec<(String, &'static str)>> {
            let requiring = Requiring::schema()
                .fields()
                .iter()
                .fold(plugin::<Requiring>(), |requiring, field| {
                    requiring.with(field.name(), requires.contains(&field.name()))
                });
            let mut draft = Draft::<C>::new();
            draft.set(&path("requirements"), requiring).unwrap();
            for field in C::schema().fields() {
                if matches!(field.kind(), FieldKind::List { .. }) {
                    draft.set(&path(field.name()), json!([])).unwrap();
                }
            }
            for (at, answer) in answers {
                draft.set(&path(at), answer.clone()).unwrap();
            }
            match finish(context, &draft) {
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
                finished::<Resolved>(|_| {}, &["downstream"], &[]),
                Ok(vec!["chaining".to_owned(), "settling".to_owned()])
            );
        }

        #[test]
        fn a_provider_whose_requirement_cannot_be_settled_is_not_added() {
            // the stranding plugin would leave the ambiguous fact unprovided
            assert_eq!(
                finished::<Resolved>(|_| {}, &["stranded"], &[]),
                Err(vec![("supplies".to_owned(), "the stranded fact")])
            );
        }

        #[test]
        fn a_provider_that_would_conflict_is_not_added() {
            let given = |declaration: &mut Declaration| {
                declaration.provide(Given);
            };

            assert_eq!(
                finished::<Resolved>(|_| {}, &["clash"], &[]),
                Ok(vec!["clashing".to_owned()])
            );
            assert_eq!(
                finished::<Resolved>(given, &["clash"], &[]),
                Err(vec![("supplies".to_owned(), "the clash fact")])
            );
        }

        #[test]
        fn an_issue_names_the_whole_chain_finishing_adds_for_it() {
            // a plugin with an unanswered field declares nothing it is configured to
            let requiring = Requiring::schema()
                .fields()
                .iter()
                .fold(plugin::<Requiring>(), |requiring, field| {
                    requiring.with(field.name(), field.name() == "downstream")
                });
            let mut draft = Draft::<Resolved>::new();
            draft.set(&path("requirements"), requiring).unwrap();
            draft.set(&path("supplies"), json!([])).unwrap();
            draft.set(&path("extras"), json!([])).unwrap();
            let added: Vec<_> = issues(|_| {}, &draft)
                .into_iter()
                .map(|issue| match issue.problem {
                    Problem::Unprovided {
                        added_by_finish, ..
                    } => added_by_finish,
                    problem => panic!("{problem:?} is not about a fact nothing provides"),
                })
                .collect();

            assert_eq!(
                added,
                [vec![
                    addition("supplies", "chaining"),
                    addition("supplies", "settling"),
                ]]
            );
        }

        #[test]
        fn a_provider_is_added_to_the_list_taking_its_axis() {
            assert_eq!(
                finished::<Resolved>(|_| {}, &["deep"], &[]),
                Ok(vec!["deepening".to_owned()])
            );
        }

        #[test]
        fn a_fact_plugins_of_two_lists_provide_is_left_unprovided() {
            assert_eq!(
                finished::<Resolved>(|_| {}, &["crosswise"], &[]),
                Err(vec![
                    ("supplies".to_owned(), "the crosswise fact"),
                    ("extras".to_owned(), "the crosswise fact"),
                ])
            );
        }

        #[test]
        fn nothing_is_added_for_a_fact_the_context_provides() {
            let context = |declaration: &mut Declaration| {
                declaration.provide(Settled);
            };

            assert_eq!(
                finished::<Resolved>(context, &["settled"], &[]),
                Ok(Vec::new())
            );
        }

        #[test]
        fn an_ambiguous_fact_is_left_unprovided_and_the_others_settled() {
            assert_eq!(
                finished::<Resolved>(|_| {}, &["ambiguous", "settled"], &[]),
                Err(vec![("supplies".to_owned(), "the ambiguous fact")])
            );
        }

        #[test]
        fn a_single_slot_is_never_added_to() {
            assert_eq!(
                finished::<Resolved>(|_| {}, &["lone"], &[]),
                Err(vec![("requirements".to_owned(), "the lone fact")])
            );
        }

        #[test]
        fn a_list_of_a_plugin_is_never_added_to() {
            assert_eq!(
                finished::<Resolved>(
                    |_| {},
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
                finished::<Twice>(|_| {}, &["settled"], &[]),
                Err(vec![
                    ("first".to_owned(), "the settled fact"),
                    ("second".to_owned(), "the settled fact"),
                ])
            );
        }
    }
}
