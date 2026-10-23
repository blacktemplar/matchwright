//! Asking the questions of a questionnaire line by line, as the page cannot wait for an answer.

use std::{cell::RefCell, fmt::Display, future, mem, rc::Rc, task::Poll};

use plugin_factory::{
    draft::Issue,
    schema::{DecimalRange, IntegerRange},
};
use tournament_cli::questionnaire::{AsyncPrompter, Choice, Error, IssueList, Question};

use crate::Reply;

/// What stops the questions when typed as an answer.
pub(crate) const CANCEL: &str = "cancel";

/// Asks by showing each question and waiting for the line [`LinePrompter::answer`] hands over. Its
/// clones share the questions and answers, so that the shell keeps one while the questionnaire
/// asks through another.
///
/// An empty line takes the answer a question offers, which the prompt shows in brackets, and
/// [`CANCEL`] stops the questions with [`Error::Cancelled`]. An answer that does not answer the
/// question is complained about, and the question waits for another one.
#[derive(Clone, Default)]
pub(crate) struct LinePrompter(Rc<RefCell<Exchange>>);

/// What a [`LinePrompter`] and the shell exchange.
#[derive(Default)]
struct Exchange {
    /// The line typed, until the question waiting for it takes it.
    answer: Option<String>,
    /// What to show since the last line was typed.
    shown: Vec<Reply>,
    /// What the prompt reads while the question waits for its answer.
    prompt: String,
}

impl LinePrompter {
    /// Hands `line` to the question waiting for it.
    pub(crate) fn answer(&self, line: &str) {
        self.0.borrow_mut().answer = Some(line.trim().to_owned());
    }

    /// What was shown since this was last called.
    pub(crate) fn shown(&self) -> Vec<Reply> {
        mem::take(&mut self.0.borrow_mut().shown)
    }

    /// What the prompt reads while the question waits for its answer, e.g. `number [3]> `.
    pub(crate) fn prompt(&self) -> String {
        self.0.borrow().prompt.clone()
    }

    fn show(&self, reply: Reply) {
        self.0.borrow_mut().shown.push(reply);
    }

    /// Shows `text` and prompts with `hint`, followed by `default` if there is one, then waits for
    /// the lines typed until one answers the question: an empty one if there is a default, else
    /// one `parse` takes.
    async fn ask<A: ToHint>(
        &self,
        text: String,
        hint: &str,
        mut default: Option<A>,
        complaint: &str,
        parse: impl Fn(&str) -> Option<A>,
    ) -> Result<A, Error> {
        self.show(Reply::Output(text));
        self.0.borrow_mut().prompt = match &default {
            Some(default) => format!("{hint} [{}]> ", default.to_hint()),
            None => format!("{hint}> "),
        };
        loop {
            let line = self.next_line().await;
            if line.eq_ignore_ascii_case(CANCEL) {
                return Err(Error::Cancelled);
            }
            if line.is_empty() {
                if let Some(default) = default.take() {
                    return Ok(default);
                }
            } else if let Some(answer) = parse(&line) {
                return Ok(answer);
            }
            self.show(Reply::Error(complaint.to_owned()));
        }
    }

    /// The next line typed. Nothing needs to be woken once it is, since the shell resumes the
    /// questions itself after handing it over.
    async fn next_line(&self) -> String {
        future::poll_fn(|_| match self.0.borrow_mut().answer.take() {
            Some(line) => Poll::Ready(line),
            None => Poll::Pending,
        })
        .await
    }
}

impl AsyncPrompter for LinePrompter {
    async fn confirm(&mut self, question: &Question, default: Option<bool>) -> Result<bool, Error> {
        self.ask(
            heading(question),
            "yes or no",
            default,
            "answer yes or no",
            |line| match line.to_lowercase().as_str() {
                "yes" | "y" => Some(true),
                "no" | "n" => Some(false),
                _ => None,
            },
        )
        .await
    }

    async fn integer(
        &mut self,
        question: &Question,
        range: &IntegerRange,
        default: Option<i64>,
    ) -> Result<i64, Error> {
        self.ask(
            ranged(question, range),
            "whole number",
            default,
            &format!("this takes {range}"),
            |line| line.parse().ok().filter(|number| range.contains(*number)),
        )
        .await
    }

    async fn decimal(
        &mut self,
        question: &Question,
        range: &DecimalRange,
        default: Option<f64>,
    ) -> Result<f64, Error> {
        self.ask(
            ranged(question, range),
            "number",
            default,
            &format!("this takes {range}"),
            |line| line.parse().ok().filter(|number| range.contains(*number)),
        )
        .await
    }

    async fn select(
        &mut self,
        question: &Question,
        options: &[Choice],
        default: Option<usize>,
    ) -> Result<usize, Error> {
        let complaint = format!(
            "answer with the number of an option, 1 to {}, or its name",
            options.len()
        );
        let chosen = self
            .ask(
                listed(question, options, None),
                "number",
                default.map(Number),
                &complaint,
                |line| option(options, line).map(Number),
            )
            .await?;
        Ok(chosen.0)
    }

    async fn select_many(
        &mut self,
        question: &Question,
        options: &[Choice],
        defaults: &[usize],
    ) -> Result<Vec<usize>, Error> {
        let complaint = format!(
            "answer with the numbers of options, 1 to {}, separated by spaces, or with none",
            options.len()
        );
        let chosen = self
            .ask(
                listed(question, options, Some(defaults)),
                "numbers",
                Some(Numbers(defaults.to_vec())),
                &complaint,
                |line| {
                    if line.eq_ignore_ascii_case("none") {
                        return Some(Numbers(Vec::new()));
                    }
                    let mut chosen = line
                        .split([' ', ','])
                        .filter(|word| !word.is_empty())
                        .map(|word| number(options, word))
                        .collect::<Option<Vec<_>>>()?;
                    chosen.sort_unstable();
                    chosen.dedup();
                    Some(Numbers(chosen))
                },
            )
            .await?;
        Ok(chosen.0)
    }

    async fn retry(&mut self, issues: Vec<Issue>) -> Result<(), Error> {
        self.show(Reply::Error(format!(
            "the answers do not fit together:{}\nplease answer again",
            IssueList(&issues)
        )));
        Ok(())
    }
}

/// How an answer is shown as the one a question offers.
trait ToHint {
    fn to_hint(&self) -> String;
}

impl ToHint for bool {
    fn to_hint(&self) -> String {
        if *self { "yes" } else { "no" }.to_owned()
    }
}

impl ToHint for i64 {
    fn to_hint(&self) -> String {
        self.to_string()
    }
}

impl ToHint for f64 {
    fn to_hint(&self) -> String {
        self.to_string()
    }
}

/// The index of an option, shown as its number, which starts at 1.
struct Number(usize);

impl ToHint for Number {
    fn to_hint(&self) -> String {
        (self.0 + 1).to_string()
    }
}

/// The indices of options, shown as their numbers.
struct Numbers(Vec<usize>);

impl ToHint for Numbers {
    fn to_hint(&self) -> String {
        if self.0.is_empty() {
            return "none".to_owned();
        }
        let numbers: Vec<_> = self.0.iter().map(|index| (index + 1).to_string()).collect();
        numbers.join(" ")
    }
}

/// The label of `question`, followed by its description, if it has one.
fn heading(question: &Question) -> String {
    match question.description() {
        Some(description) => format!("{}\n  {description}", question.label()),
        None => question.label().to_owned(),
    }
}

/// The heading of `question`, followed by what it takes, `range`, e.g. "takes a number of at least
/// 0".
fn ranged(question: &Question, range: &impl Display) -> String {
    format!("{}\n  takes {range}", heading(question))
}

/// The heading of `question`, followed by its numbered `options`. A question taking several of
/// them has `chosen`, and marks each option as chosen or not.
fn listed(question: &Question, options: &[Choice], chosen: Option<&[usize]>) -> String {
    let mut text = heading(question);
    for (index, option) in options.iter().enumerate() {
        let mark = match chosen {
            None => "",
            Some(chosen) if chosen.contains(&index) => "[x] ",
            Some(_) => "[ ] ",
        };
        text.push_str(&format!("\n  {mark}{}. {option}", index + 1));
    }
    text
}

/// The index of the option `line` answers with, by its number or by its label.
fn option(options: &[Choice], line: &str) -> Option<usize> {
    number(options, line).or_else(|| {
        options
            .iter()
            .position(|option| option.label().eq_ignore_ascii_case(line))
    })
}

/// The index of the option numbered `word`.
fn number(options: &[Choice], word: &str) -> Option<usize> {
    let number: usize = word.parse().ok()?;
    (1..=options.len()).contains(&number).then(|| number - 1)
}
