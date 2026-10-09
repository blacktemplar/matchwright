# Proposal: plugins describe themselves

Status: proposal. To be folded into `DESIGN.md` once settled.

## The problem

A UI that lets someone configure a phase has to know every plugin. The CLI
knows them four times over: `phase.rs` has an enum variant per plugin with its
label, description, default parameters and dependencies; `wizard.rs` has a
`match` arm per plugin asking for its parameters; `session.rs` has
`METRIC_COLUMNS`, a header and a formatter per metric. The engine mirrors a
piece of it in `DefaultProvider`, where a core fact type names a concrete
plugin and its default parameters.

So adding a plugin means editing four tables, and a third-party plugin is
invisible to the CLI.

## The idea

Each plugin ships a **description of its own configuration**: what to call it,
what to ask for, and how to turn the answers into an instance. Plugins register
themselves in a **catalog** (via `inventory`). A UI reads the catalog and the
descriptions; it never names a plugin.

Two things follow from making that usable:

- later questions depend on earlier answers ("which metrics?" depends on the
  format), so something has to hold the half-configured phase → a **builder**;
- the engine can now *see* every plugin, so it can find the calculator a
  missing metric needs by itself → **resolution** replaces `DefaultProvider`.

## How it works, by example: adding a round robin

### 1. The UI asks the phase what it needs

`PhasePlugins` describes itself the same way a plugin does — three fields: one
phase format, one ranker, any number of metrics calculators. The CLI starts a
builder for it and loops over the questions it hands out:

```rust
let mut builder = Builder::<PhasePlugins>::new(competition.phase_context());
while let Some(question) = builder.questions().into_iter().next() {
    let answer = ask(&question, builder.candidates(&question.path));
    builder.set(&question.path, answer)?;
}
let plugins = builder.finish()?;
```

`ask` is the only CLI-side code: it renders a label, a help text, a default and
(for a plugin field) a list of candidates. It never mentions a plugin by name.

### 2. "Phase format" — the candidates come from the catalog

The builder lists every registered `PhaseFormat`, each with the label and
description from its own registration, sorted by id. Monster DYP comes back
marked as conflicting, because it requires rankings of single players and the
previous phase produced pairs — the UI greys it out and shows that sentence.

The operator picks round robin.

### 3. Picking a plugin can add questions

Round robin has no parameters, so nothing is added. Had they picked single
elimination, its own description would have added "Play a match for third
place?" to the question list, which is why the loop re-reads `questions()`
after every answer.

### 4. "Metrics to calculate" — the default comes from the format

The default of this field is *what the configured plugins recommend*. Round
robin's description recommends wins/draws/losses, points and Sonneborn-Berger —
the knowledge `FormatConfig::default_metrics` holds today, but living in the
format plugin, so a third-party format brings its own.

The operator accepts. Each chosen calculator now contributes its own questions:
"Points for a win", offered with `1.0` because `PointsCalculator`'s description
says so. That default used to exist twice, as `DEFAULT_POINTS` in the CLI and
inside `Points::add_default_provider`.

### 5. "Ranker" — a default can be a whole configured subtree

Round robin recommends a complete
`HierarchicalRanker [ LexicographicalCriteria [ MorePoints, MoreSonnebornBerger ] ]`.
The operator accepts it as one answer and never sees the nesting. Someone who
wants to change it drills in, and the criteria are then just another plugin
field with its own candidates — the same mechanism, one level down.

### 6. `finish` builds, and the engine fills the gaps

Sonneborn-Berger requires the `Matches` metric, which nobody asked for.
Resolution looks through the catalog, finds exactly one calculator that provides
it and needs no configuration, and adds it. Then it sorts the calculators so
each runs after the ones it reads, and `AddPhase` checks the result as it does
today. The CLI's `derived_from` table is gone.

Had two calculators provided `Matches`, resolution could not have chosen, and
the metrics field would have been marked while configuring instead, with those
two highlighted in the list it already offers.

## The pieces

Catalog — a registry per axis:

- **`PluginKind`** — a marker type per axis (`MetricsCalculatorKind`, …) tying
  the axis trait to its registry. `inventory::collect!` needs a type local to
  the collecting crate, so a macro generates the registry type; that is the only
  reason the macro exists.
- **`PluginEntry<K>`** — one plugin's registration: `id` (stable, namespaced,
  the same string as its `typetag` tag), `label`, `description`, and the erased
  form of what `Configurable` authors.
- **`Kind`** — an erased handle (`Kind::of::<K>()`) so a schema can say "a
  plugin of this kind" and the builder can list that kind's entries.
- **`EntryView`** — the object-safe view of an entry that the builder and a UI
  browse through. It can build an instance internally and hand back only the
  small result — a `Declaration`, a list of recommendations. Construction stays
  typed: `Config::plugins::<K>()` returns `Vec<Box<dyn SliceRanker>>`, nothing
  to downcast.

Description — what to ask, how to build, what fits alongside:

- **`Configurable`** — `schema()`, `build(&Config)`, and a defaulted
  `recommends(&self)`. Implemented by plugins *and* by `PhasePlugins`, which is
  what makes the loop above work for a phase and for a nested
  `HierarchicalRanker` alike.
- **`Schema` / `Field` / `FieldKind`** — a list of fields; a field is a bool, an
  integer, a decimal, one plugin of a kind, or several plugins of a kind. That
  is all the current plugins need.
- **`Value` / `Config` / `PluginConfig`** — the answers: a tree of plain data,
  `serde`-serializable, keyed by field name. This is what makes CLI presets and
  headless wizard tests possible.
- **`ConfigError`** — one error type for everything that can go wrong while
  configuring: unknown field or plugin, wrong kind of value, a value a plugin
  rejects, a question left unanswered, an incompatibility. It carries the `Path`
  of the field it is about.

Builder — the half-configured composite:

- **`Builder<T>`** — holds the answers, hands out `questions()`, `default(path)`,
  `candidates(path)` and `issues()`, and `finish()`es into `T`.
- **`Path`** — addresses a field in the tree (field name, list index, repeat for
  nesting). One builder owns the whole tree rather than one per nested plugin,
  so that a changed answer can invalidate what depended on it.
- **`Question`**, **`Candidate`** — what a UI renders: a field with its default;
  an entry with "fits" or "conflicts, because …", and whether it is recommended.
- **`Issue`** — something currently wrong, with the `Path` of the field where it
  gets fixed: either an answer that no longer fits, or a requirement nothing
  provides, together with the entries that would.

Engine side:

- **`Accepts<K>`** — implemented by `PhasePlugins` for the kinds resolution may
  add to it. A phase accepts calculators and nothing else, so "resolution adds a
  second phase format" cannot be written. It replaces
  `DefaultProvider::add_default_provider(&mut PhasePlugins)`, which made a fact
  type know both a plugin and the composite.

That is 18 new items plus the per-kind markers. Step 1 of the build order needs
four of them; the rest is the price of a UI that can ask for parameters.

## Defaults and resolution

Two mechanisms, and the CLI's tables split along the line between them: round
robin's *default* metrics include Sonneborn-Berger because it is worth showing;
*resolution* adds `Matches` because Sonneborn-Berger cannot be computed without
it, whether anyone wanted it or not.

| | question | lives in | binding? |
| --- | --- | --- | --- |
| default | what do I offer as the answer? | the plugin's own description, or the siblings' recommendations | no, overridable |
| resolution | what does this phase *need* to work? | facts + `Accepts` | yes, correctness |

**Every field has a default, from one of two sources.** A plugin's own scalar
parameters carry a constant: `PointsCalculator` says `1.0` for a win,
`SingleElimination` says yes to a match for third place. Nobody else has an
opinion about those, and they are most fields. A composite's plugin fields — the
metrics and the ranker of a phase — have no sensible constant, so their default
is what the configured siblings **recommend**: each configured plugin offers a
list of plugin configurations, and the field keeps the ones of its own kind,
dropping those that conflict, first recommendation winning.

That is the only cross-field default, and it is why recommending sits on the
plugin rather than on the composite. Keyed by kind, a format never learns what
`PhasePlugins` calls its fields, so a third-party composite gets the same
recommendations; and owning its own preset is what lets a third-party format
have one at all. The alternative — one computed default in `PhasePlugins` —
would need facts like "every entrant meets every other" invented purely to
carry taste, in a vocabulary meant for correctness.

**Requirements do not feed defaults.** A required fact with exactly one
automatic provider is resolution's business and stays invisible: nobody is asked
about `Matches`, just as today's CLI keeps it out of `SELECTABLE`. A required
fact with no automatic provider, or with several, becomes an `Issue` instead —
see below.

## How the builder behaves

**An answer is an answer, whoever came up with it.** A field is either answered
or not; accepting a default is the same as typing it, so the builder stores
answers only. Defaults are computed when a field is asked, and once more for
whatever is unanswered at `finish` — never stored, so never stale. Step 4 above
works because the metrics are asked *after* the format is set, not because
anything is recomputed behind the operator's back.

An answer is therefore never silently changed. A later answer can *invalidate*
an earlier one — change the format and a metric may no longer fit — which
becomes an `Issue` for the operator to resolve. A UI that wants a changed answer
to refresh the ones after it unsets them explicitly, which puts their defaults
back in play.

**Candidates are optimistic about what is still open.** A candidate conflicts
when a fact it provides is already provided, when a condition it requires is
false for a fact a *configured* plugin provides, or when it requires a fact
nothing available could ever provide. Anything that merely depends on a slot
still to be filled is offered. Being wrong that way costs a revised answer;
being wrong the other way hides a plugin with no way to find out why. The
existing `check_facts` in `AddPhase::check_applicable` stays the authority, so
optimism cannot let an invalid phase through.

**Everything wrong is attached to a field.** `set` checks the value against the
field, lets the plugin's own `build` validate the subtree, and checks the choice
against the facts configured so far, so a bad value is reported at once and by
path. Afterwards `issues()` reports, also by path, the earlier answers that no
longer fit and the requirements nothing provides yet. For a missing fact the
builder asks each plugin-valued field whether its kind has an entry providing it
— for `Matches` that is the metrics field and nothing else, so that field is
marked "a calculator providing `Matches` is missing" together with the ids that
would. Those entries are in its candidate list already, since they fit, so the
operator picks one and its questions open like any other choice. Nothing needs a
flow of its own.

If several fields qualify the issue goes on all of them, since any would do. If
*none* does, it is not a question but an unsatisfiable configuration, reported as
an error: nothing anywhere can provide that fact. A UI cannot get there, because
the candidate filter would not have offered the requiring plugin; a preset file
loaded from disk can.

### Wizards and forms

The builder is pull-shaped on purpose: `questions()`, `default(path)` and
`candidates(path)` are queries, not a protocol. A wizard calls them one question
at a time; a form calls them for everything it renders and `set`s on submit.
Nothing in the model assumes a sequence. What a form needs beyond that:

- **The layout is known before anything is answered.** The top level of the
  question tree is the composite's schema, which is fixed. Deeper questions
  appear only as a consequence of the operator's own choice of plugin, and an
  entry's schema can be read *before* that plugin is picked, so a form can say
  in advance what picking it will ask for.
- **Which answers are mandatory is in the schema**: the fields without a
  default. Every current plugin parameter has one, so that set is empty today.
- **Stable identity and order.** A question is identified by its `Path` and
  ordered by the schema, depth-first, so re-reading `questions()` never
  reshuffles what the operator is looking at, and a form can key its widgets by
  path.
- **Nothing to show sits outside that layout**, because every error and every
  issue carries the path of the field it belongs to.

What cannot be reported *early*, in any UI: "nothing provides fact X" only
becomes true or false as the configuration fills up, so it appears as an issue
while configuring and, if ignored, at `finish`. That is the price of optimism
about open slots.

## What does not change

- The transition log keeps storing built trait objects through `typetag`.
  Schemas and answer trees live between a UI and a constructor; they are not a
  second persistence format.
- Resolution stays in `AddPhase::new`, never in `apply`. Its result depends on
  which crates are linked, so it must not be a replay input — the log holds the
  finished, sorted plugin list, as today.
- Nothing here runs while a competition is played. All of it is once per phase.
- `Plugin::declare` and the facts are unchanged. `requires_with_default` goes
  away, because the catalog finds providers without a trait impl to pick up.

## Build order

Each step is shippable on its own, and nothing in the log changes.

0. **Write the declarations.** Today every `declare` only provides a
   calculator's own metric, and the formats declare nothing: `SonnebornBerger`
   does not say it reads `Points` and `Matches`, the elimination formats do not
   say they eliminate, monster DYP does not say it needs single players. Nothing
   below works without them — not ordering the calculators, not adding `Matches`
   by itself, not filtering what a UI offers. One prerequisite for all three,
   and it needs none of the new types.
1. **Catalog, no schemas.** `PluginKind`, `PluginEntry` with just
   `{ id, label, description, default: Option<fn() -> Box<K::Object>> }`,
   `Accepts<K>`, the macro. Switch resolution to the catalog; delete
   `DefaultProvider`, `requires_with_default` and the CLI's `derived_from`. The
   whole engine-side win, with four new types and no schemas.
2. **Schemas.** `Configurable`, `Schema`/`Field`/`Value`/`Config`, `Kind`,
   `EntryView`; register the existing plugins. Still no UI.
3. **`impl Configurable for PhasePlugins`** plus `recommends` on the formats,
   pinned by a test that accepting every default reproduces today's presets
   exactly.
4. **`Builder`.** Testable against the existing plugins before any UI uses it.
5. **The CLI.** Replace `wizard.rs` with the loop, delete `phase.rs`'s enums.
6. **Metric descriptions**, replacing `METRIC_COLUMNS`: a label and a formatter
   registered next to each metric type, so a third-party metric gets a column.
   Independent of everything above.
7. **Optional: a derive macro** for the registration boilerplate, once the entry
   shape has stopped moving.

## Deferred, all additive

- **A preview of what resolution will add**, greyed into the field it goes into.
  The pass the builder already runs computes it and throws it away, so this is
  one more path-keyed query — `issues()` cannot cover it, since that would mean
  no longer settling anything automatically. If it is added: run the whole
  resolution loop on a clone, because an added plugin can require facts itself;
  recompute per query like a default; attach it by the same field lookup an
  issue uses, which is unambiguous as long as each accepted kind has exactly one
  field — a conformance test, not a flag on the field. It must not go into the
  answer tree, which would reintroduce a provenance per field and bake today's
  resolution decisions into saved presets.
- **A field for a closed set of labelled choices, and for a string.** No current
  plugin has either, and anything with parameters of its own is better off as a
  nested kind.
- **`describe`, the inverse of `build`.** Nothing can edit a phase — there is no
  `ChangePhase` transition — and the builder holds the answers while
  configuring.
- **An explicit "never create this unasked" flag.** Derived for now from "every
  field has a default".
- **A default computed by an arbitrary closure**, and with it the `Partial` view
  of the half-configured phase that such a closure would need. Recommendations
  cover the cross-field case, and a plugin's recommendations depend only on its
  own configuration (single elimination recommends `FewerLosses` only when it
  plays for third place), so no plugin author writes a closure. The one case
  that might ask for a computed scalar is a field following another field of the
  same plugin — "points for a bye" offering what was answered for a win — which
  the constant `1.0` covers today. Deriving the rounds of a Swiss phase from the
  entrant count, the other candidate, is not available at all: a phase is
  configured before the competition starts, and the entrant count is only known
  at `StartCompetition`.
- **Preference among several providers of one metric**, the planned
  `offers_calculator`. Two providers are an ambiguity a UI can ask about, and
  the candidate filter usually removes the wrong one.

## Rejected

- **Generating the CLI enums** from the plugin list. A consumer-side table
  either way, and no third-party plugin ever appears in it.
- **A serde-derived schema** (`schemars` or similar), building with
  `from_value`. Deletes `Field`, `Value` and `Config`, but cannot express "a
  plugin of kind `SliceRanker`" without a custom extension, and has nowhere to
  put a default that comes from a sibling's recommendation — the two features
  the design exists for.
- **One global registry** with `build` returning `Box<dyn Any>`. One `collect!`
  instead of a generated type per kind, but trades a compile-time guarantee for
  a downcast.
- **A plugin that adds itself to the composite** (today's `DefaultProvider`).
  Makes every plugin know `PhasePlugins`, and cannot express "this slot is
  single, nothing may be added to it".
- **Nested builders** instead of paths. Each would need its ancestors' facts, so
  either it borrows its parent — and a UI cannot hold two open — or facts get
  copied and a changed ancestor cannot invalidate them.
- **Pessimistic candidate filtering** (the full `check_facts`). Nothing could be
  offered until everything was configured.
- **Defaults keyed on sibling plugin ids** (`if format == SingleElimination`).
  The CLI's table moved one crate over, still blind to third-party formats.
- **Defaults filled in and recomputed whenever anything changes**, with a
  provenance per field telling an operator's answer from a filled-in one.
  Accepting a default is answering, so there is nothing to recompute, and
  computing defaults on demand removes the state entirely.
- **A separate list of unresolved requirements** next to the questions. Every
  one of them is fixed in some field, so it belongs on that field; a UI then has
  nothing outside the schema's layout to render.

## Open questions

- `Competition::declare_phase_context` is crate-private. `Builder::new` needs
  it, so it has to become public (filling it in is step 0).
- Two configured plugins recommending different things for one single-plugin
  field: first in schema order wins. Enough, or do recommendations need a
  weight? Nothing in the current plugin set hits this.
- Should a candidate distinguish "fits" from "fits only if some open slot
  provides X", so a UI can sort rather than treat all candidates alike?
- Who owns UI text? `&'static str` on the plugin means the engine ships
  English. That is what the CLI does today; localisation would wrap it, keyed by
  plugin id.
- Is a saved answer tree (a CLI preset file) a supported format? If yes, plugin
  ids *and* field names get the same stability contract as `typetag` tags.
