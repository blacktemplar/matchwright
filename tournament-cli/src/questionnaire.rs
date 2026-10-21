//! Asking for the answers to the schema of any composite of plugins, question by question.
//!
//! [`fill`] walks the schema of a [`Configurable`] along a [`Draft`] of it, and the schema of every
//! plugin chosen on the way, and asks one question per field through a [`Prompter`]: whether for a
//! yes or no, a number, one plugin of an axis, or a list of answers. It knows no plugin and no
//! composite; what it offers comes from the catalog and from the draft.
//!
//! Every question starts out at the answer the field already has in the draft, or else at the
//! answer the draft offers for it, see [`Draft::default_value`], so accepting every question
//! answers the composite as its plugins recommend. `Terminal` (feature `terminal`) asks at the
//! terminal, and [`AcceptDefaults`] accepts every offered answer without asking anything.

use std::{
    fmt::{self, Display, Formatter},
    pin::pin,
    task::{Context, Poll, Waker},
};

#[cfg(feature = "terminal")]
use inquire::{Confirm, CustomType, InquireError, MultiSelect, Select};
use plugin_factory::{
    catalog::{AnyAxis, PluginEntry},
    config::{ConfigError, Path, PluginConfig, Segment, Value},
    draft::{Candidate, Draft, Issue, Problem},
    schema::{Configurable, Field, FieldKind},
};
use thiserror::Error;

/// What the option of a list of plugins that ends it is shown as.
const NO_FURTHER_ONE: &str = "(no further one)";

/// What the option of a field that may take no plugin that takes none is shown as.
const NONE: &str = "(none)";

/// One question, as a [`Prompter`] shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    label: String,
    description: Option<&'static str>,
}

impl Question {
    /// What is asked.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// What the answer means, if anything is said about it.
    pub fn description(&self) -> Option<&'static str> {
        self.description
    }

    /// This question with `label` instead of its label.
    fn relabelled(&self, label: String) -> Self {
        Self {
            label,
            description: self.description,
        }
    }
}

/// One option of a question asking for plugins: the plugin, by label and description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    label: &'static str,
    description: Option<&'static str>,
}

impl Choice {
    fn of(entry: &PluginEntry) -> Self {
        Self {
            label: entry.label(),
            description: entry.description(),
        }
    }

    /// An option that stands for no plugin, e.g. for ending a list.
    fn nothing(label: &'static str) -> Self {
        Self {
            label,
            description: None,
        }
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn description(&self) -> Option<&'static str> {
        self.description
    }
}

/// Shows the label, followed by the description if there is one.
impl Display for Choice {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self.description {
            Some(description) => write!(formatter, "{} — {description}", self.label),
            None => formatter.write_str(self.label),
        }
    }
}

/// Asks the questions [`fill`] has, each starting out at the answer it offers, if any, and returns
/// the answer right away.
pub trait Prompter {
    /// Asks for yes or no.
    ///
    /// # Errors
    /// Returns an error if the question is not answered.
    fn confirm(&mut self, question: &Question, default: Option<bool>) -> Result<bool, Error>;

    /// Asks for a whole number.
    ///
    /// # Errors
    /// Returns an error if the question is not answered.
    fn integer(&mut self, question: &Question, default: Option<i64>) -> Result<i64, Error>;

    /// Asks for a number.
    ///
    /// # Errors
    /// Returns an error if the question is not answered.
    fn decimal(&mut self, question: &Question, default: Option<f64>) -> Result<f64, Error>;

    /// Asks for one of `options`, which are never empty, and returns its index.
    ///
    /// # Errors
    /// Returns an error if the question is not answered.
    fn select(
        &mut self,
        question: &Question,
        options: &[Choice],
        default: Option<usize>,
    ) -> Result<usize, Error>;

    /// Asks for any number of `options` and returns their indices, in ascending order.
    ///
    /// # Errors
    /// Returns an error if the question is not answered.
    fn select_many(
        &mut self,
        question: &Question,
        options: &[Choice],
        defaults: &[usize],
    ) -> Result<Vec<usize>, Error>;

    /// Tells that the answers given have `issues`, before the fields they are in are asked again.
    ///
    /// # Errors
    /// Returns an error, e.g. [`Error::Issues`], if the fields are not to be asked again.
    fn retry(&mut self, issues: Vec<Issue>) -> Result<(), Error>;
}

/// Asks the questions [`fill_async`] has like a [`Prompter`], but may wait for the answers, e.g.
/// for an operator typing them into a page, which cannot block until they do. Every [`Prompter`]
/// is one.
pub trait AsyncPrompter {
    /// Asks for yes or no, see [`Prompter::confirm`].
    fn confirm(
        &mut self,
        question: &Question,
        default: Option<bool>,
    ) -> impl Future<Output = Result<bool, Error>>;

    /// Asks for a whole number, see [`Prompter::integer`].
    fn integer(
        &mut self,
        question: &Question,
        default: Option<i64>,
    ) -> impl Future<Output = Result<i64, Error>>;

    /// Asks for a number, see [`Prompter::decimal`].
    fn decimal(
        &mut self,
        question: &Question,
        default: Option<f64>,
    ) -> impl Future<Output = Result<f64, Error>>;

    /// Asks for one of `options`, see [`Prompter::select`].
    fn select(
        &mut self,
        question: &Question,
        options: &[Choice],
        default: Option<usize>,
    ) -> impl Future<Output = Result<usize, Error>>;

    /// Asks for any number of `options`, see [`Prompter::select_many`].
    fn select_many(
        &mut self,
        question: &Question,
        options: &[Choice],
        defaults: &[usize],
    ) -> impl Future<Output = Result<Vec<usize>, Error>>;

    /// Tells that the answers given have `issues`, see [`Prompter::retry`].
    fn retry(&mut self, issues: Vec<Issue>) -> impl Future<Output = Result<(), Error>>;
}

impl<P: Prompter> AsyncPrompter for P {
    async fn confirm(&mut self, question: &Question, default: Option<bool>) -> Result<bool, Error> {
        Prompter::confirm(self, question, default)
    }

    async fn integer(&mut self, question: &Question, default: Option<i64>) -> Result<i64, Error> {
        Prompter::integer(self, question, default)
    }

    async fn decimal(&mut self, question: &Question, default: Option<f64>) -> Result<f64, Error> {
        Prompter::decimal(self, question, default)
    }

    async fn select(
        &mut self,
        question: &Question,
        options: &[Choice],
        default: Option<usize>,
    ) -> Result<usize, Error> {
        Prompter::select(self, question, options, default)
    }

    async fn select_many(
        &mut self,
        question: &Question,
        options: &[Choice],
        defaults: &[usize],
    ) -> Result<Vec<usize>, Error> {
        Prompter::select_many(self, question, options, defaults)
    }

    async fn retry(&mut self, issues: Vec<Issue>) -> Result<(), Error> {
        Prompter::retry(self, issues)
    }
}

/// Why the questions of a schema were not all answered.
#[derive(Debug, Error)]
pub enum Error {
    /// The operator cancelled the questions.
    #[error("the questions were cancelled")]
    Cancelled,
    /// The terminal did not answer, other than because the operator cancelled the question.
    #[cfg(feature = "terminal")]
    #[error(transparent)]
    Prompt(InquireError),
    /// [`AcceptDefaults`] has no answer to give, because the question offers none.
    #[error("{question:?} offers no answer, so it has to be answered")]
    NoDefault { question: String },
    /// No plugin of the field's axis fits next to the answers given so far.
    #[error("no plugin fits {path} next to the answers given so far")]
    NoCandidate { path: Path },
    /// The answers given do not make a composite, and are not asked again.
    #[error("the answers do not fit together:{}", IssueList(.0))]
    Issues(Vec<Issue>),
    /// An answer is not one of its field, which only a schema whose plugins disagree with it
    /// causes.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

#[cfg(feature = "terminal")]
impl From<InquireError> for Error {
    fn from(error: InquireError) -> Self {
        match error {
            InquireError::OperationCanceled | InquireError::OperationInterrupted => Self::Cancelled,
            error => Self::Prompt(error),
        }
    }
}

/// Asks for every field of `draft` that is not to be answered by the operator differently, through
/// `prompter`, and answers it in the draft.
///
/// The fields are asked in schema order, each followed by the fields of the plugin it is answered
/// with, if any. A question starts out at the field's answer in `draft`, so that filling a draft
/// once more only asks for changes, or else at the answer [`Draft::default_value`] offers.
///
/// A field taking plugins offers those that fit next to the answers given before it, see
/// [`Draft::candidates`]. A field taking an unordered list of plugins starts out with the plugins
/// it offers and those it is the only source of something another plugin requires, see
/// [`Candidate::is_sole_supplier`]. The plugins a question starts out at come first, in the order
/// they are offered in, so that what the plugins chosen before recommend leads; the others follow
/// alphabetically by label.
///
/// Answers a plugin rejects are handed to [`Prompter::retry`] as soon as the fields of the plugin
/// are answered, and its fields are asked again. Once every field is answered, plugins that do not
/// fit next to the others are handed to it as well, and the fields of the composite they are in
/// are asked again. A fact nothing provides is not checked, since a plugin it implies may still be
/// added, see [`Draft::add_implied`].
///
/// # Errors
/// Returns what the prompter returns, and [`Error::NoCandidate`] if no plugin fits a field taking
/// one. The draft keeps the answers given so far.
pub fn fill<T: Configurable>(
    draft: &mut Draft<T>,
    prompter: &mut impl Prompter,
) -> Result<(), Error> {
    let mut context = Context::from_waker(Waker::noop());
    match pin!(fill_async(draft, prompter)).poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => unreachable!("a `Prompter` answers right away, so filling never waits"),
    }
}

/// [`fill`], through a prompter that may wait for the answers.
///
/// # Errors
/// As [`fill`].
pub async fn fill_async<T: Configurable>(
    draft: &mut Draft<T>,
    prompter: &mut impl AsyncPrompter,
) -> Result<(), Error> {
    let schema = T::schema();
    let mut asked: Vec<&Field> = schema.fields().iter().collect();
    loop {
        let mut asker = Asker { draft, prompter };
        for field in asked {
            asker.field(field, &Path::ROOT, None).await?;
        }
        let issues: Vec<_> = draft
            .issues()
            .into_iter()
            .filter(|issue| !matches!(issue.problem(), Problem::Unprovided { .. }))
            .collect();
        if issues.is_empty() {
            return Ok(());
        }
        asked = schema
            .fields()
            .iter()
            .filter(|field| {
                issues.iter().any(|issue| {
                    issue.path().segments().first()
                        == Some(&Segment::Field(field.name().to_owned()))
                })
            })
            .collect();
        // an issue not within a field is fixed by none of them in particular
        if asked.is_empty() {
            asked = schema.fields().iter().collect();
        }
        prompter.retry(issues).await?;
    }
}

/// `answer`, an answer to a field taking plugins of `axis`, as it reads in a summary: the label of
/// the plugin followed by the answers to its fields in parentheses, if it has any; [`None`] if it
/// is no plugin of the axis.
///
/// # Examples
/// ```
/// use plugin_factory::{catalog::AnyAxis, config::PluginConfig};
/// use tournament_cli::questionnaire;
/// use tournament_engine::plugins::phase_format::{PhaseFormat, RandomRounds};
///
/// let answer = PluginConfig::of::<RandomRounds>().with("rounds", 5).into();
///
/// assert_eq!(
///     questionnaire::describe(AnyAxis::of::<dyn PhaseFormat>(), &answer).as_deref(),
///     Some("random rounds (rounds: 5)")
/// );
/// ```
pub fn describe(axis: AnyAxis, answer: &Value) -> Option<String> {
    let plugin = PluginConfig::from_answer(answer)?;
    let entry = axis.entry(plugin.plugin())?;
    let answers: Vec<_> = entry
        .schema()
        .fields()
        .iter()
        .filter_map(|field| {
            let answer = plugin.config().get(field.name())?;
            Some(format!(
                "{}: {}",
                field.label(),
                describe_answer(field.kind(), answer)
            ))
        })
        .collect();
    if answers.is_empty() {
        return Some(entry.label().to_owned());
    }
    Some(format!("{} ({})", entry.label(), answers.join(", ")))
}

/// `answer`, an answer of `kind`, as it reads in a summary.
fn describe_answer(kind: &FieldKind, answer: &Value) -> String {
    match (kind, answer) {
        (_, Value::Null) => "none".to_owned(),
        (_, Value::Bool(true)) => "yes".to_owned(),
        (_, Value::Bool(false)) => "no".to_owned(),
        (FieldKind::Optional(inner), _) => describe_answer(inner, answer),
        (FieldKind::List { axis, .. }, Value::Array(answers)) => {
            let answers: Vec<_> = answers
                .iter()
                .map(|answer| describe(*axis, answer).unwrap_or_else(|| answer.to_string()))
                .collect();
            format!("[{}]", answers.join(", "))
        }
        (FieldKind::Plugin(axis), _) => {
            describe(*axis, answer).unwrap_or_else(|| answer.to_string())
        }
        _ => answer.to_string(),
    }
}

/// The answer of `kind` that `text` writes, as an operator types it: `yes` or `no` (or `true` or
/// `false`) for yes or no, a number for a number, and `none` for nothing where a field takes it;
/// [`None`] if `text` writes none, and for a kind of answer that is not typed, such as a plugin.
///
/// # Examples
/// ```
/// use plugin_factory::{config::Value, schema::FieldKind};
/// use tournament_cli::questionnaire::parse_answer;
///
/// assert_eq!(
///     parse_answer(&FieldKind::Bool, "no"),
///     Some(Value::from(false))
/// );
/// assert_eq!(parse_answer(&FieldKind::Integer, "7"), Some(Value::from(7)));
/// assert_eq!(parse_answer(&FieldKind::Integer, "seven"), None);
/// ```
pub fn parse_answer(kind: &FieldKind, text: &str) -> Option<Value> {
    match kind {
        FieldKind::Bool => match text.to_lowercase().as_str() {
            "yes" | "true" => Some(Value::from(true)),
            "no" | "false" => Some(Value::from(false)),
            _ => None,
        },
        FieldKind::Integer => text.parse::<i64>().ok().map(Value::from),
        FieldKind::Decimal => text
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(Value::from),
        FieldKind::Optional(_) if text.eq_ignore_ascii_case("none") => Some(Value::Null),
        FieldKind::Optional(inner) => parse_answer(inner, text),
        FieldKind::Plugin(_) | FieldKind::List { .. } => None,
    }
}

/// Issues, each on a line of its own, as [`Error::Issues`] shows them.
pub struct IssueList<'a>(pub &'a [Issue]);

impl Display for IssueList<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        for issue in self.0 {
            write!(formatter, "\n  {issue}")?;
        }
        Ok(())
    }
}

/// Asks at the terminal.
#[cfg(feature = "terminal")]
pub struct Terminal;

#[cfg(feature = "terminal")]
impl Prompter for Terminal {
    fn confirm(&mut self, question: &Question, default: Option<bool>) -> Result<bool, Error> {
        let prompt = Confirm {
            default,
            help_message: question.description(),
            ..Confirm::new(question.label())
        };
        Ok(prompt.prompt()?)
    }

    fn integer(&mut self, question: &Question, default: Option<i64>) -> Result<i64, Error> {
        ask_number(question, default, "this takes a whole number")
    }

    fn decimal(&mut self, question: &Question, default: Option<f64>) -> Result<f64, Error> {
        ask_number(question, default, "this takes a number")
    }

    fn select(
        &mut self,
        question: &Question,
        options: &[Choice],
        default: Option<usize>,
    ) -> Result<usize, Error> {
        let prompt = Select {
            starting_cursor: default.unwrap_or(0),
            help_message: question.description(),
            ..Select::new(question.label(), options.to_vec())
        };
        Ok(prompt.raw_prompt()?.index)
    }

    fn select_many(
        &mut self,
        question: &Question,
        options: &[Choice],
        defaults: &[usize],
    ) -> Result<Vec<usize>, Error> {
        let help = match question.description() {
            Some(description) => format!("{description}; space toggles, enter confirms"),
            None => "space toggles, enter confirms".to_owned(),
        };
        let chosen = MultiSelect::new(question.label(), options.to_vec())
            .with_default(defaults)
            .with_help_message(&help)
            .raw_prompt()?;
        Ok(chosen.into_iter().map(|option| option.index).collect())
    }

    fn retry(&mut self, issues: Vec<Issue>) -> Result<(), Error> {
        eprintln!(
            "the answers do not fit together:{}\nplease answer again",
            IssueList(&issues)
        );
        Ok(())
    }
}

/// Asks `question` at the terminal for a number, repeating it with `error_message` until the
/// answer is one.
#[cfg(feature = "terminal")]
fn ask_number<T: Clone + std::str::FromStr + ToString>(
    question: &Question,
    default: Option<T>,
    error_message: &str,
) -> Result<T, Error> {
    let prompt = CustomType {
        default,
        help_message: question.description(),
        ..CustomType::new(question.label()).with_error_message(error_message)
    };
    Ok(prompt.prompt()?)
}

/// Accepts the answer every question offers, without asking anything.
///
/// It fails at a question that offers none, see [`Error::NoDefault`], and at answers with issues,
/// see [`Error::Issues`].
pub struct AcceptDefaults;

impl AcceptDefaults {
    fn accept<T>(question: &Question, default: Option<T>) -> Result<T, Error> {
        default.ok_or_else(|| Error::NoDefault {
            question: question.label().to_owned(),
        })
    }
}

impl Prompter for AcceptDefaults {
    fn confirm(&mut self, question: &Question, default: Option<bool>) -> Result<bool, Error> {
        Self::accept(question, default)
    }

    fn integer(&mut self, question: &Question, default: Option<i64>) -> Result<i64, Error> {
        Self::accept(question, default)
    }

    fn decimal(&mut self, question: &Question, default: Option<f64>) -> Result<f64, Error> {
        Self::accept(question, default)
    }

    fn select(
        &mut self,
        question: &Question,
        _: &[Choice],
        default: Option<usize>,
    ) -> Result<usize, Error> {
        Self::accept(question, default)
    }

    fn select_many(
        &mut self,
        _: &Question,
        _: &[Choice],
        defaults: &[usize],
    ) -> Result<Vec<usize>, Error> {
        Ok(defaults.to_vec())
    }

    fn retry(&mut self, issues: Vec<Issue>) -> Result<(), Error> {
        Err(Error::Issues(issues))
    }
}

/// Asks the fields of one draft, see [`fill`].
struct Asker<'a, T, P> {
    draft: &'a mut Draft<T>,
    prompter: &'a mut P,
}

impl<T: Configurable, P: AsyncPrompter> Asker<'_, T, P> {
    /// Asks `field` of the config at `at`, which is that of the plugin labelled `plugin` unless it
    /// is the composite's own.
    async fn field(&mut self, field: &Field, at: &Path, plugin: Option<&str>) -> Result<(), Error> {
        let path = at.clone().field(field.name());
        let label = match plugin {
            Some(plugin) => format!("{plugin}: {}", field.label()),
            None => field.label().to_owned(),
        };
        let question = Question {
            label: capitalized(&label),
            description: field.description(),
        };
        let default = match self.draft.answers().get_at(&path) {
            Some(answer) => Some(answer.clone()),
            None => self.draft.default_value(&path)?,
        };
        // boxed, since answering a plugin asks for its fields in turn
        Box::pin(self.answer(field.kind(), &path, &question, default)).await
    }

    /// Asks `question` for the answer of `kind` at `path`, starting out at `default`.
    async fn answer(
        &mut self,
        kind: &FieldKind,
        path: &Path,
        question: &Question,
        default: Option<Value>,
    ) -> Result<(), Error> {
        let answer = match kind {
            FieldKind::Bool => {
                let default = default.as_ref().and_then(Value::as_bool);
                Value::from(self.prompter.confirm(question, default).await?)
            }
            FieldKind::Integer => {
                let default = default.as_ref().and_then(Value::as_i64);
                Value::from(self.prompter.integer(question, default).await?)
            }
            FieldKind::Decimal => {
                let default = default.as_ref().and_then(Value::as_f64);
                Value::from(self.prompter.decimal(question, default).await?)
            }
            FieldKind::Plugin(_) => return self.plugin(path, question, default, false).await,
            FieldKind::Optional(inner) => match **inner {
                FieldKind::Plugin(_) => return self.plugin(path, question, default, true).await,
                _ => {
                    let answered = self
                        .prompter
                        .confirm(
                            &question.relabelled(format!("{}: give an answer?", question.label())),
                            Some(default.as_ref().is_some_and(|default| !default.is_null())),
                        )
                        .await?;
                    if answered {
                        let default = default.filter(|default| !default.is_null());
                        return Box::pin(self.answer(inner, path, question, default)).await;
                    }
                    Value::Null
                }
            },
            FieldKind::List { ordered, .. } => {
                let defaults = match default {
                    Some(Value::Array(defaults)) => defaults,
                    _ => Vec::new(),
                };
                self.draft.set(path, Value::Array(Vec::new()))?;
                return if *ordered {
                    self.plugin_list(path, question, &defaults).await
                } else {
                    self.plugin_set(path, question, &defaults).await
                };
            }
        };
        self.draft.set(path, answer)?;
        Ok(())
    }

    /// The plugins that fit at `path`, see [`Draft::candidates`], those of `first` first, see
    /// [`ordered`].
    fn candidates(&self, path: &Path, first: &[&str]) -> Result<Vec<Candidate>, Error> {
        let candidates = self
            .draft
            .candidates(path)?
            .into_iter()
            .filter(|candidate| candidate.conflicts().is_empty())
            .collect();
        Ok(ordered(candidates, first))
    }

    /// Asks `question` for the plugin at `path`, or for none if `optional`, starting out at
    /// `default`.
    async fn plugin(
        &mut self,
        path: &Path,
        question: &Question,
        default: Option<Value>,
        optional: bool,
    ) -> Result<(), Error> {
        let default_plugin = default.as_ref().and_then(PluginConfig::from_answer);
        let first: Vec<_> = default_plugin.iter().map(PluginConfig::plugin).collect();
        let candidates = self.candidates(path, &first)?;
        let mut options: Vec<_> = candidates
            .iter()
            .map(|candidate| Choice::of(candidate.entry()))
            .collect();
        let mut cursor = default_plugin.as_ref().and_then(|default| {
            candidates
                .iter()
                .position(|candidate| candidate.entry().id() == default.plugin())
        });
        if optional {
            if default.as_ref().is_some_and(Value::is_null) {
                cursor = Some(options.len());
            }
            options.push(Choice::nothing(NONE));
        }
        if options.is_empty() {
            return Err(Error::NoCandidate { path: path.clone() });
        }
        let chosen = self.prompter.select(question, &options, cursor).await?;
        match candidates.get(chosen) {
            Some(candidate) => {
                self.put(candidate.entry(), path, default_plugin.as_ref())
                    .await
            }
            None => Ok(self.draft.set(path, Value::Null)?),
        }
    }

    /// Asks `question` for the plugins of the unordered list at `path`, starting out with
    /// `defaults` and the plugins that are the only source of something another one requires.
    async fn plugin_set(
        &mut self,
        path: &Path,
        question: &Question,
        defaults: &[Value],
    ) -> Result<(), Error> {
        let defaults = plugins(defaults);
        let at = path.clone().index(0);
        // what the list offers, then what only one plugin supplies to another
        let mut first: Vec<_> = defaults.iter().map(PluginConfig::plugin).collect();
        for candidate in self.candidates(&at, &[])? {
            let id = candidate.entry().id();
            if !first.contains(&id) && candidate.is_sole_supplier() {
                first.push(id);
            }
        }
        let candidates = self.candidates(&at, &first)?;
        let options: Vec<_> = candidates
            .iter()
            .map(|candidate| Choice::of(candidate.entry()))
            .collect();
        let preselected: Vec<_> = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| first.contains(&candidate.entry().id()))
            .map(|(index, _)| index)
            .collect();
        if options.is_empty() {
            return Ok(());
        }
        let chosen = self
            .prompter
            .select_many(question, &options, &preselected)
            .await?;
        for (position, index) in chosen.into_iter().enumerate() {
            let entry = candidates[index].entry();
            let default = defaults
                .iter()
                .find(|default| default.plugin() == entry.id());
            self.put(entry, &path.clone().index(position), default)
                .await?;
        }
        Ok(())
    }

    /// Asks `question` for the plugins of the ordered list at `path` one after another, until the
    /// operator asks for no further one or none fits, each starting out at the first of `defaults`
    /// that fits.
    async fn plugin_list(
        &mut self,
        path: &Path,
        question: &Question,
        defaults: &[Value],
    ) -> Result<(), Error> {
        let defaults = plugins(defaults);
        let first: Vec<_> = defaults.iter().map(PluginConfig::plugin).collect();
        for position in 0.. {
            let at = path.clone().index(position);
            let candidates = self.candidates(&at, &first)?;
            if candidates.is_empty() {
                break;
            }
            let options: Vec<_> = candidates
                .iter()
                .map(|candidate| Choice::of(candidate.entry()))
                .chain([Choice::nothing(NO_FURTHER_ONE)])
                .collect();
            // a plugin in the list already is no candidate any more, so this is the first default
            // still to be added
            let cursor = defaults
                .iter()
                .find_map(|default| {
                    candidates
                        .iter()
                        .position(|candidate| candidate.entry().id() == default.plugin())
                })
                .unwrap_or(candidates.len());
            let question = if position == 0 {
                question.clone()
            } else {
                question.relabelled(format!("{}, then", question.label()))
            };
            let chosen = self
                .prompter
                .select(&question, &options, Some(cursor))
                .await?;
            let Some(candidate) = candidates.get(chosen) else {
                break;
            };
            let default = defaults
                .iter()
                .find(|default| default.plugin() == candidate.entry().id());
            self.put(candidate.entry(), &at, default).await?;
        }
        Ok(())
    }

    /// Answers `path` with the plugin `entry`, configured as `default` if that is the same plugin,
    /// and asks for the fields of the plugin.
    async fn put(
        &mut self,
        entry: &'static PluginEntry,
        path: &Path,
        default: Option<&PluginConfig>,
    ) -> Result<(), Error> {
        let answer = match default {
            Some(default) if default.plugin() == entry.id() => default.clone(),
            _ => PluginConfig::new(entry.id()),
        };
        self.draft.set(path, answer)?;
        loop {
            for field in entry.schema().fields() {
                self.field(field, path, Some(entry.label())).await?;
            }
            // checked right away, since a plugin that cannot be built recommends nothing for the
            // questions after it
            let rejected: Vec<_> = self
                .draft
                .issues()
                .into_iter()
                .filter(|issue| {
                    matches!(issue.problem(), Problem::Invalid(_))
                        && issue.path().segments().starts_with(path.segments())
                })
                .collect();
            if rejected.is_empty() {
                return Ok(());
            }
            self.prompter.retry(rejected).await?;
        }
    }
}

/// `label` with its first letter in upper case, as a question starts.
fn capitalized(label: &str) -> String {
    let mut chars = label.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// `candidates` in the order they are offered in: those whose id is in `first`, in that order,
/// then the others by label.
fn ordered(mut candidates: Vec<Candidate>, first: &[&str]) -> Vec<Candidate> {
    // the catalog lists plugins by id, which tells an operator nothing
    candidates.sort_by_cached_key(|candidate| {
        let entry = candidate.entry();
        (
            first
                .iter()
                .position(|id| *id == entry.id())
                .unwrap_or(usize::MAX),
            entry.label().to_lowercase(),
        )
    });
    candidates
}

/// The plugins among `answers`.
fn plugins(answers: &[Value]) -> Vec<PluginConfig> {
    answers
        .iter()
        .filter_map(PluginConfig::from_answer)
        .collect()
}

/// The questionnaire of the plugins of a phase, which is what the CLI asks it for.
#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, fmt::Debug};

    use serde::Deserialize;
    use tournament_engine::{Competition, PhasePlugins};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    /// How a [`Script`] answers a question.
    enum Step {
        /// The answer the question offers.
        Accept,
        Integer(i64),
        /// The option with this label.
        Pick(&'static str),
    }

    /// Answers the questions with its steps in turn, and accepts every question once they are
    /// used up.
    #[derive(Default)]
    struct Script {
        steps: VecDeque<Step>,
        /// The labels of the questions asked, in order.
        asked: Vec<String>,
        /// The labels of the options of every question asking for plugins, in order.
        options: Vec<Vec<&'static str>>,
        retries: usize,
    }

    impl Script {
        fn new(steps: impl IntoIterator<Item = Step>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
                ..Self::default()
            }
        }

        fn next(&mut self, question: &Question) -> Step {
            self.asked.push(question.label().to_owned());
            self.steps.pop_front().unwrap_or(Step::Accept)
        }

        fn offered<D: Debug>(question: &Question, default: Option<D>) -> D {
            default.unwrap_or_else(|| panic!("{:?} offers no answer", question.label()))
        }
    }

    impl Prompter for Script {
        fn confirm(&mut self, question: &Question, default: Option<bool>) -> Result<bool, Error> {
            match self.next(question) {
                Step::Accept => Ok(Self::offered(question, default)),
                _ => panic!("{:?} takes yes or no", question.label()),
            }
        }

        fn integer(&mut self, question: &Question, default: Option<i64>) -> Result<i64, Error> {
            match self.next(question) {
                Step::Accept => Ok(Self::offered(question, default)),
                Step::Integer(answer) => Ok(answer),
                Step::Pick(_) => panic!("{:?} takes a whole number", question.label()),
            }
        }

        fn decimal(&mut self, question: &Question, default: Option<f64>) -> Result<f64, Error> {
            match self.next(question) {
                Step::Accept => Ok(Self::offered(question, default)),
                _ => panic!("{:?} is only accepted", question.label()),
            }
        }

        fn select(
            &mut self,
            question: &Question,
            options: &[Choice],
            default: Option<usize>,
        ) -> Result<usize, Error> {
            self.options
                .push(options.iter().map(Choice::label).collect());
            match self.next(question) {
                Step::Accept => Ok(Self::offered(question, default)),
                Step::Pick(label) => Ok(options
                    .iter()
                    .position(|option| option.label() == label)
                    .unwrap_or_else(|| panic!("{:?} offers no {label:?}", question.label()))),
                Step::Integer(_) => panic!("{:?} takes an option", question.label()),
            }
        }

        fn select_many(
            &mut self,
            question: &Question,
            options: &[Choice],
            defaults: &[usize],
        ) -> Result<Vec<usize>, Error> {
            self.options
                .push(options.iter().map(Choice::label).collect());
            match self.next(question) {
                Step::Accept => Ok(defaults.to_vec()),
                _ => panic!("{:?} is only accepted", question.label()),
            }
        }

        fn retry(&mut self, _: Vec<Issue>) -> Result<(), Error> {
            self.retries += 1;
            Ok(())
        }
    }

    /// An empty draft of a phase of a new competition.
    fn phase_draft() -> Draft<PhasePlugins> {
        Competition::default().phase_draft()
    }

    /// The ids of the plugins answering the list `field` of `draft`, without their namespace.
    fn ids(draft: &Draft<PhasePlugins>, field: &str) -> Vec<String> {
        plugins(draft.answers().get(field).unwrap().as_array().unwrap())
            .iter()
            .map(|plugin| {
                plugin
                    .plugin()
                    .trim_start_matches("tournament_engine::")
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn accepting_every_question_configures_what_the_format_recommends() {
        let mut script = Script::new([Step::Pick("round robin")]);
        let mut draft = phase_draft();

        fill(&mut draft, &mut script).unwrap();

        assert_eq!(
            script.asked,
            [
                "Format",
                "Ranked by",
                "Ranked by, then",
                "Ranked by, then",
                "Metrics",
                "Points: points for a win",
                "Points: points for a draw",
                "Points: points for a bye",
                "Sonneborn-Berger: weight of a beaten opponent",
                "Sonneborn-Berger: weight of an opponent played to a draw",
            ]
        );
        assert_eq!(
            ids(&draft, "ranking"),
            ["more_points", "more_sonneborn_berger"]
        );
        // in the order the format recommends them
        assert_eq!(
            ids(&draft, "metrics"),
            ["wins_draws_losses", "points", "sonneborn_berger"]
        );
        assert!(draft.build().is_ok());
    }

    /// The plugins a question starts out at lead, in the order the plugins chosen before
    /// recommend them, and the others follow by label, whatever their ids.
    #[test]
    fn the_plugins_recommended_come_first_and_the_others_by_label() {
        let mut script = Script::new([Step::Pick("round robin")]);

        fill(&mut phase_draft(), &mut script).unwrap();

        assert_eq!(
            script.options[0],
            [
                "round robin",
                "double elimination",
                "Monster DYP",
                "random rounds",
                "single elimination"
            ]
        );
        assert_eq!(
            script.options[1][..4],
            [
                "more points",
                "more Sonneborn-Berger",
                "fewer losses",
                "fewer matches played"
            ]
        );
        assert_eq!(
            script.options[4][..4],
            [
                "win-draw-loss record",
                "points",
                "Sonneborn-Berger",
                "Buchholz"
            ]
        );
    }

    #[test]
    fn a_question_starts_with_a_capital_letter() {
        assert_eq!(
            capitalized("random rounds: rounds"),
            "Random rounds: rounds"
        );
        assert_eq!(capitalized(""), "");
    }

    /// A metric is offered as calculated once something ranks by it, if a single calculator
    /// calculates it.
    #[test]
    fn a_metric_the_ranking_requires_starts_out_chosen() {
        let mut script = Script::new([
            Step::Pick("round robin"),
            Step::Pick("more Buchholz"),
            Step::Pick(NO_FURTHER_ONE),
        ]);
        let mut draft = phase_draft();

        fill(&mut draft, &mut script).unwrap();

        assert_eq!(ids(&draft, "ranking"), ["more_buchholz"]);
        assert_eq!(
            ids(&draft, "metrics"),
            [
                "wins_draws_losses",
                "points",
                "sonneborn_berger",
                "buchholz"
            ]
        );
    }

    /// A rejected answer is asked again right away, so that the questions after it start out at
    /// what the plugin recommends once it is configured.
    #[test]
    fn an_answer_a_plugin_rejects_is_asked_again_before_the_next_question() {
        let mut script = Script::new([
            Step::Pick("random rounds"),
            Step::Integer(-1),
            Step::Integer(3),
        ]);
        let mut draft = phase_draft();

        fill(&mut draft, &mut script).unwrap();

        assert_eq!(script.retries, 1);
        assert_eq!(
            script.asked[..4],
            [
                "Format",
                "Random rounds: rounds",
                "Random rounds: rounds",
                "Ranked by"
            ]
        );
        assert_eq!(
            draft.answers().get_at(&"format.rounds".parse().unwrap()),
            Some(&Value::from(3))
        );
        assert_eq!(
            ids(&draft, "ranking"),
            [
                "more_points",
                "more_buchholz",
                "more_sonneborn_berger",
                "more_buchholz_of_buchholz"
            ]
        );
    }

    /// Filling a draft once more starts out at its answers, so accepting every question keeps it
    /// as it is.
    #[test]
    fn a_filled_draft_is_kept_by_accepting_every_question() {
        let mut draft = phase_draft();
        fill(
            &mut draft,
            &mut Script::new([
                Step::Pick("Monster DYP"),
                Step::Integer(7),
                Step::Pick("more wins"),
            ]),
        )
        .unwrap();
        let answers = draft.answers().clone();

        fill(&mut draft, &mut AcceptDefaults).unwrap();

        assert_eq!(draft.answers(), &answers);
        assert_eq!(ids(&draft, "ranking")[0], "more_wins");
    }

    #[test]
    fn accepting_every_question_needs_each_to_offer_an_answer() {
        /// A composite whose only field offers no answer.
        #[derive(Deserialize, Configurable)]
        struct Rounds {
            #[plugin(label = "rounds")]
            _rounds: u32,
        }

        let result = fill(&mut Draft::<Rounds>::new(|_| {}), &mut AcceptDefaults);

        assert!(
            matches!(&result, Err(Error::NoDefault { question }) if question == "Rounds"),
            "{result:?}"
        );
    }
}
