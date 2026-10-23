# Tournament Engine — Design Plan

A generic core library for running 1-on-1 match tournaments (single elimination,
round-robin, Swiss, ...), where key components are pluggable and selected at
runtime.

Sections marked *planned* describe decisions that are not implemented yet.

## Global constraints

These apply to every mechanism below and are not restated per section.

- **Runtime selection.** Which implementation of a pluggable axis is used is
  chosen at runtime, not baked in by monomorphization.
- **Third-party openness.** A crate outside this one must be able to add an
  implementation of any axis, a new metric, or new consumer data, without
  modifying this crate and without special-casing.
- **Two persistence paths.** A `Competition` is persistable as a snapshot, and
  (*planned*) the transition log is persistable too. Both must survive a
  recompile, a dependency bump or a redeploy, as long as the concrete types
  involved are still linked in. Nothing process-local (e.g. raw
  `std::any::TypeId`) may appear in a serialized form.
- **Determinism under replay.** Progress is a log of transitions replayed from
  the initial state (see "Progression as an undoable state machine"), so
  `Transition::apply` must be a pure function of state and the transition's own
  data. Anything derived from the clock, the environment or an external service
  cannot be computed in `apply`; it has to arrive as transition data.
- **No untyped data bags.** `HashMap<String, Value>` is rejected wherever it
  comes up: it turns wrong keys and wrong shapes into runtime failures, and it
  cannot carry behavior. Where open-ended data is genuinely needed, it is keyed
  by Rust type instead (see `TypeMap`).

## Pluggable axes

The library is built around independent axes of behavior — the phase format
(single elimination, round-robin, Swiss, ...), the ranker, the metrics
calculators, and more as they turn up. Writing an implementation for one axis
must never require touching another axis's trait.

### Decision

Every axis is its own public, unsealed trait, selected as `Box<dyn Trait>`.
Implemented axes: `PhaseFormat`, `SliceRanker` and `MetricsCalculator`. The
mechanics of an axis come from the `plugin-factory` crate: the trait is declared
with `#[axis]` and has `plugin_factory::Plugin` as a supertrait, and a plugin
derives `Plugin` and registers its impl of the axis trait with `#[register]`
(see [The plugin catalog](#the-plugin-catalog)). On top of that, every axis
trait of the engine has `DynClone` as a supertrait and implements `Clone` for
`Box<dyn Trait>` with `dyn_clone::clone_trait_object!`, since `AddPhase::apply` clones the plugins on
every replay.

**The ranker is not an axis.** A phase is always ranked by a
`HierarchicalRanker`, a plain struct the phase builds from its list of
`SliceRanker`s, each breaking the ties the ones before it left. Every way of
ranking that was foreseen is such a chain, so a second axis above `SliceRanker`
only added a layer of trait objects and tags. A slice ranker sorting by a single
metric — a *criterion* such as `MorePoints` — implements the helper trait
`RankingCriterion` (its metric type and a comparison), declares
`declare_static = requires_metric::<Self>` and calls `rank_by` from its
`SliceRanker` impl; other slice rankers (e.g. head-to-head) implement
`SliceRanker` directly.

This applies uniformly to future axes: each one gets a trait and a slot, with no
per-axis judgment call.

### Alternatives

- **Closed `Kind` enum per axis.** Allows static dispatch via `enum_dispatch`,
  but third parties cannot add variants. Rejected.
- **Generic type parameters per axis.** Zero-cost, but conflicts with runtime
  selection, and every combination becomes a distinct type. Rejected.
- **Name-driven registry from the start** (`inventory`/`linkme`). Not needed to
  select an axis, since a consumer can construct the implementation it wants.
  Added later for what does need it, see
  [The plugin catalog](#the-plugin-catalog).
- **The engine's own axis machinery** (the previous design): a hand-written
  `Plugin` supertrait with a required `declare`, `plugin_kind!` generating the
  registry of an axis, and an engine-side compatibility check and resolution.
  Superseded by `plugin-factory`, which does the same for any composite and adds
  schemas, configuration and drafts.

## Plugin state

A plugin may need memory beyond what the matches and rankings record, e.g. a
precomputed bracket or a structure it would otherwise rebuild from every match on
every call — and those calls run on every reported result.

### Decision

**Plugins own their state and update it through `&mut self`.** The methods that
follow the phase's progress take `&mut self` and receive the phase's data rather
than the phase:

```rust
fn create_new_matches(&mut self, phase_data: &PhaseData) -> Schedule;      // PhaseFormat
fn update_ranks(&mut self, phase_data: &PhaseData,
                slice: &mut [(&Ranking, NonZeroU32)]);                     // SliceRanker
fn update_after_match_result(&mut self, phase_data: &PhaseData, /* ... */)
    -> Vec<(RankingId, Metric)>;                                           // MetricsCalculator
```

`HierarchicalRanker::update_ranks` takes `&mut self` too, since it calls its
slice rankers. Methods that only describe a plugin's configuration
(`default_metric_value`, `allows_draws`) stay `&self`.

**State is a field left out of the schema.** Everything `plugin-factory` sees of
a plugin is its configuration, so a field holding state, such as the bracket of
`SingleElimination`, is marked `#[plugin(skip)]` and needs a
`#[serde(default)]`: a plugin built from a configuration starts with the
default, and a saved one comes back with the state it had. The field is always
serialized, `None` included, since `skip_serializing_if` breaks postcard, which
reads fields by position; instead a configuration answering a skipped field is
not rejected, but the answer is dropped.

**Undo needs nothing extra.** A plugin is a field of its `Phase`, so its state is
part of the competition state: it is in a snapshot, and undo — a replay of the
remaining log from the initial state — rebuilds the plugin from its initial
configuration and runs it through the same calls again. The only contract is
determinism under replay applied to plugins: a plugin's state may depend only on
the `PhaseData` and the calls it has seen.

**`PhaseData` is everything in a `Phase` but its plugins** — rankings, their
order, matches and the seed. The split is what makes `&mut self` possible: a call
borrows a plugin field mutably and `data` immutably, two disjoint fields of the
same `Phase`, so no plugin has to be moved out for the call. The consequence is
that a plugin no longer sees the other plugins of its phase; what it needs from
them reaches it through `PhaseData` (metrics on rankings, annotations on matches).
Consumers read the same `PhaseData` through `Phase::data`. `data` serializes as a nested field rather than with
`#[serde(flatten)]`, because flattening goes through serde's self-describing map
path, which postcard cannot deserialize.

### Alternatives

- **Stateless plugins (`&self`, `&Phase`)**, recomputing whatever they need from
  the phase on every call (the previous design). Keeps plugins trivially pure, but
  makes the cost of a call grow with the phase on the result-reporting hot path,
  which is the argument already made for incremental metrics. Superseded.
- **Plugin state stored in `PhaseData`**, e.g. a `TypeMap` keyed by a state type,
  with plugins staying `&self`. Separates state from the code that owns it,
  exposes it to every other plugin, and two plugins of the same type would collide
  on the key. Rejected.
- **Interior mutability** (`RefCell` inside the plugin) behind `&self`. Hides the
  mutation from the signature while the replay contract still applies. Rejected.
- **Passing `&Phase` with the plugin temporarily taken out** (`Option` or
  `mem::replace`). The plugin would observe a phase missing one of its plugins, a
  state that exists nowhere else. Rejected.
- **Pure configuration plugins building a runtime object** (e.g.
  `PhaseFormat::start` returning a separately tagged scheduler holding the
  state). Separates configuration from state cleanly, but costs a second trait
  and tag per stateful axis. Not chosen; a skipped field does the same within
  one struct.
- **Serializing state only when it is not the default**
  (`skip_serializing_if`), so that a drafted plugin never carries it. Breaks
  non-self-describing formats. Rejected.
- **Rebuilding state lazily from `PhaseData`** after a load (`#[serde(skip)]`).
  Every stateful plugin would need a replay of its own history. Rejected.

## Compatibility between plugins

*Implemented: the mechanism in `plugin-factory` (`compatibility`, `draft`), the
engine's facts in `plugins::facts`, `PhasePlugins`, `AddPhase`, and every plugin
of this workspace declaring what it provides and requires.*

Not every combination of plugins is valid, and detecting that must not rely on a
hardcoded list of concrete type pairs, which would be blind to third-party
implementations. The dependencies that already exist or are foreseeable run
between every pair of axes and in both directions:

- a calculator reads another calculator's metric (Points on wins/draws/losses,
  Buchholz on Points);
- a calculator reads match annotations only some formats write (round of
  elimination on `Eliminates`);
- a criterion sorts by a metric some calculator must produce;
- a calculator needs matches of a certain structure (goal difference needs
  goals, provided by a future match plugin axis);
- a format needs something of the ranker (Swiss pairs by rank) and a ranker of
  the format (ranking by elimination round needs an elimination format);
- a format needs something of the phase's input (monster DYP forms teams per
  match from rankings of single players, so it cannot follow a DYP draw that
  already made each ranking a pair).

A phase is a loop — the format schedules, results arrive, calculators update,
the ranker ranks, the format schedules again from those ranks — so there is no
pipeline direction these could be sorted into. Two concerns are kept apart:
whether a set of plugins fits together (*compatibility*), and in which order
plugins called one after another within a single pass must run (*execution
order*).

### Decision

**Compatibility is checked on typed facts.** A fact is a value of a type
implementing the marker trait `plugin_factory::compatibility::Fact` (`'static +
Debug + Sync`), describing something true of a phase and identified by its
`TypeId`. A plugin states what it provides and requires in a `plugin-factory`
`Declaration`: `declare_static` for what every configuration of it declares,
which the catalog reads before the plugin is configured, and `declare` for what
depends on its configuration, both given to the derive:

```rust
impl Declaration {
    pub fn provide<T>(&mut self, value: T) -> &mut Self;
    pub fn require<T>(&mut self, name: &'static str) -> &mut Self;  // present at all
    pub fn require_that<T>(&mut self, name: &'static str, description: &'static str,
                           holds: fn(&T) -> bool) -> &mut Self;
}
```

What a plugin declares includes what the plugins in its fields declare, so a
composite plugin forwards nothing by hand. The `HierarchicalRanker` is no
plugin; each slice ranker of `PhasePlugins` is checked on its own, so an issue
names the slice ranker that has it.

A criterion requires its metric through `requires_metric::<Self>`, which reads
the requirement `CalculatesMetric<C::Metric>` off `C::Metric`, the metric type
the criterion compares and `rank_by` reads.

**Facts are opted into.** `Fact` has no blanket impl; a type becomes a fact only
by an explicit `impl Fact for T {}`. Rejected: any `'static + Debug + Sync` type
being a fact. That let a plugin require a type that is never provided as a
fact, e.g. a metric itself instead of its `CalculatesMetric`, which compiled and
only failed the check as unmet. And by the orphan rule a crate can only make its
own types facts, so two crates cannot collide on a shared foreign type such as
`bool` or `u32`: every fact belongs to the crate owning its concept. A derive
for `Fact` is not worth it while the trait is an empty marker; it becomes one
if a fact grows something to state, e.g. the name a requirement now passes by
hand.

Facts are provided by the phase's plugins and by its context: the competition,
declared by `Competition::declare_phase_context`, e.g. `EntrantsPerRanking(1)`.
A plugin providing a fact the context provides already is an incompatibility.
The check collects all provided facts into a type-keyed map built only for the
check (never stored or serialized) and evaluates every requirement against it.
It is flat: any plugin may require a fact of any other plugin or of the context,
and mutual requirements are fine, because a requirement states what must be true
of the phase, not what must have run first.

The vocabulary is open. Core defines facts about concepts it owns
(`MatchAnnotation<A>`, `EntrantsPerRanking`, ...); the crate
that owns a concept defines facts about it (a match plugin crate defines
`MatchDetail<Goals>`, which a goal-difference calculator in another crate
requires). Incompatibilities are unmet conditions on facts — monster DYP
`require_that::<EntrantsPerRanking>(.., "rankings of single players", ..)` —
never pairs of concrete types. A fact carries a value, so a requirement can ask
*how* as well as *whether*, and a fact holding a `fn` or a `Box<dyn Trait>`
carries behavior where a plugin needs some from another.

A fact provided twice within a phase is an error.

**Calculating a metric is a fact**, `CalculatesMetric<M>`, and a calculator is
an ordinary plugin declaring that it provides it
(`provide(CalculatesMetric::<Points>::new())`). There is no separate handling: a
metric calculated by two calculators is a fact provided twice (the disjointness
of metrics), and a required metric no calculator calculates is an unmet fact.
Rejected: the metric type itself as the fact, provided as its default value.
That value meant nothing, yet a requirement could state a condition on it, and
what a calculator promises is that every ranking will hold the metric, not a
value of it. A metric type is no `Fact`, so requiring it by mistake does not
compile. That the declared metric is the one `default_metric_value()` returns is
checked by a test over every registered calculator; `plugin-factory` does not
expose a plugin's declaration for a runtime assertion.

**The mechanism knows no axes.** `plugin-factory` holds declarations, the check,
a generic order of any list of plugins and the plugins missing facts imply. The
one place that knows the axes of a phase is `PhasePlugins`, a `Configurable`
composite of one format, a list of slice rankers and a list of calculators.

**Self-reported.** Every declaration, a criterion's included, is stated by the
plugin. That is the trade-off accepted here: facts like "rankings are pairs" are
not something the compiler can derive, and none of the alternatives can express
conditions on values. A criterion's requirement at least cannot drift from what
it compares, since `requires_metric` reads it off `C::Metric`, but a criterion
that leaves out `declare_static` requires nothing, and `rank_by` panics when the
phase runs. Test helpers that build a phase from a plugin plus only the facts it
declares catch reads it did not declare.

**Execution order is fixed by the engine, except among calculators.** On a
reported result the engine runs the calculators, then the ranker, and the format
schedules next; scheduled matches and their annotations exist before any of
their results. So the ranker always sees current metrics, the format current
ranks, and calculators every annotation; none of these needs ordering. The only
place plugins run one after another within a pass and read what an earlier one
wrote in that same pass is the calculator list, so only it is sorted: a
calculator requiring a fact runs after the calculator providing it, ties keep
the configured order (deterministic, so replay is unaffected), and a cycle is an
error. Should a future axis also become a list of plugins building on each
other within a pass, the same rule applies to it.

**Running order is an invariant of a set of plugins.** The calculators of a phase
are a `plugin_factory::schema::DependencyOrderedSet`, since their order means
nothing but the order they run in, and a set of plugins keeps itself in that
order: `DependencyOrderedSet::new` and deserializing a set both sort it, and fail
on a cycle. Both also reject a
plugin given twice, whatever it is configured with, so a phase cannot calculate
a metric with two configurations of one calculator. Deserializing is
how `Draft::build` builds a composite, and how a saved `AddPhase` comes back,
so neither building from a draft nor loading a log needs a step of its own. The sort keeps an order that is already valid as it is, so a
saved set replays in the order it was saved in, and one in an order its
calculators cannot run in, e.g. edited by hand, is put into running order rather
than run wrong. The slice rankers stay a `Vec`, since their order is the
ranking.

**Missing providers come from the catalog: implied plugins.** For a required
fact nothing provides, `Draft::add_implied` looks among the axes of the
composite's lists of plugins for the registered plugins that provide it and have
no fields, and adds the one it finds if there is exactly one — the plugin the
fact implies, since there is neither a choice nor anything to configure. The
win-draw-loss record has exactly one calculator, so nobody is ever asked about
it. Several candidates are an ambiguity it must not decide, so it adds nothing
and the fact stays unprovided, as it does for a fact only a plugin with fields
provides: a default a plugin offers is never filled in unasked. A field taking
one plugin, such as the format, is never added to. Adding repeats until nothing
changes, see below. A plugin
configured explicitly always wins, since the fact it provides is never missing.
Consumers only configure calculators that need their configuration (points per
win, ...) or that they want calculated without ranking by them. A UI does not
add a provider with fields behind the operator's back either; it offers it, as
the CLI's questionnaire does by starting out with it chosen.

**A fact implies its plugins all or nothing.** What an added plugin requires and
nothing provides implies plugins the same way, in turn, and the plugins found for one missing
fact are added only together and only if none of them raises an issue: a fact
provided twice, a fact still missing, a condition not met — on what they
require or on what they provide to a plugin already there — or a cycle.
Otherwise nothing is added and the original fact stays unprovided, an issue at
the place the operator configured. So an automatic addition never leaves behind
a problem with a plugin nobody chose, e.g. a calculator that cannot read a
third-party match format. Checking a chain needs the phase's context, since a requirement the context
meets is no issue. Problems among the configured plugins themselves do not
block an addition. The additions are not reported ahead of adding them: no
consumer showed them, and computing them for every candidate and issue made
`draft` the most complex part of `plugin-factory`. A candidate only reports
whether it fits (`Candidate::conflicts`) and whether it is the only plugin
supplying something the others require (`Candidate::is_sole_supplier`).

**Adding the implied plugins and building are separate steps.**
`Draft::add_implied` appends the implied plugins to the answers, and
`Draft::build` builds the composite from the answers as they are, adding
nothing; since it changes nothing, it borrows the draft. Whether to add the
implied plugins is the consumer's choice, and the answers it reads afterwards
are exactly what the composite is built from, e.g. to describe the phase it
adds. Building without them reports the facts they would have provided as
unprovided. `add_implied` tries every missing fact
again after each addition, since an addition may provide what the chain of a
fact tried before it lacked, and stops once a whole pass adds nothing, so
calling it twice adds nothing the first call did not. Since an addition stays an
answer even if the plugin requiring it is taken back later, a consumer whose
draft may still change adds them to a clone, as the CLI's `Session::add_phase`
does. Rejected: building adding them to a copy, with nothing public (the
answers a phase was built from were unobservable); building adding them as well
as a public `add_implied` (hid the choice, and two ways to the same result);
and trying each fact once per call (an earlier fact a later addition made
implying stayed missing, so a second call could add more). The name: the
plugins are implied by the other answers — exactly one way to provide a missing
fact and nothing to configure — which neither "settle" nor "resolve
dependencies" says.

**Implied plugins are added before the transition, validation is in it.**
Adding them is the consumer's step: it adds them to the `Draft<PhasePlugins>` of
`Competition::phase_draft`, which carries the competition's phase context, and
builds the plugins from it.
In production, building from that draft is the only way to make `PhasePlugins`
besides deserializing them. Code configuring a phase answers the draft with
configs (`Draft::with`, `Draft::with_defaults`) just like a UI does.

**Tests build plugins directly, behind the `test-util` feature.** The
constructors of plugins with fields (`PointsCalculator::new`, ...), their
`with_..` methods, `PhasePluginsBuilder` and `PhaseBuilder` (a standalone
`Phase` for unit tests of plugins, built on `PhasePluginsBuilder`) exist only
with the feature, which the engine's tests, benchmarks and examples enable
through a dev-dependency on the crate itself. A test that is not about
configuration names only the plugins it cares about, with no configs to spell
out by field name; builders rather than helper functions, so that whatever a
test leaves out comes from a default and what it names is what it is about.
What a test leaves out of the builder is empty, apart from the format, which is
a round robin: an empty ranking is a valid one that leaves every entrant tied,
so `PhasePluginsBuilder::new().build()` is a phase that can be played, and a
test that does not care about the ranking or the metrics does not name any.
Rejected: defaulting to a ranking by points with the calculators it needs,
which took rules of its own for what to add and when, for no test that needed
it; and `PhasePlugins::new(format, ranking, metrics)` next to the builder, a
second way to the same result in which every part has to be given. The builder
adds no implied plugins, which only a draft adds. Tests of configuration, of implied plugins,
and of a consumer's configuring (the CLI's `Session::add_phase` takes a draft)
still use drafts. A consumer enables the feature only in its dev-dependencies,
so production code has no way to make plugins that skips their configuration.
`AddPhase` validates directly built plugins like any others. The
transition holds the complete, sorted list, so neither the log nor a snapshot
depends on how calculators were picked — which it must not, because the catalog
depends on which crates are linked in — and changing a registration or the way
of picking later never changes a replay. Deserializing an `AddPhase` adds
nothing — a missing calculator fails the replay. Facts are checked in
`AddPhase::check_applicable`, where the phase's context is known; facts only
known at start, such as the number of entrants, are checked the same way at
`StartCompetition`. All of this runs once
per phase; the result-reporting path loops over an already sorted list and pays
nothing.

*Planned, once a metric has a second calculator:* a preference among the
candidates for one metric, so that a fact can imply a plugin where it is an
ambiguity today — e.g. a match format offering the points calculator that
understands its matches (`Plugin::recommends` covers the offered default
already). This is additive, and since adding implied plugins is not part of the
log it can change freely.

### Alternatives

- **Marker traits registered with `intertrait`** (the previous plan). Providing
  is structural, but it answers only "does some plugin implement `XYZ`": it
  cannot state conditions on values ("rankings are single players"), needs a
  link-time registry, and cannot register generic impls. A fact holding a trait
  object covers the behavioral case without it. Superseded.
- **Closed `Capability` enum owned by core.** Cheap checks, but the vocabulary is
  closed to core. Rejected.
- **Hardcoded match over concrete type pairs.** Invisible to third-party types.
  Rejected.
- **Fixed stages with requirements only on earlier stages** (match plugin →
  calculators → ranker → format). Ties compatibility to execution order, which a
  phase does not have between axes: the format needs the ranker and the ranker
  the format. Rejected.
- **Calculators in the order configured, unchecked** (the current state).
  Pushes a hidden ordering contract onto every consumer, e.g. the CLI's
  `derived_from` table. Superseded.
- **Consumers adding every needed calculator themselves.** Each consumer would
  need its own table of which metric derives from which, like the CLI's
  `derived_from`. Superseded by the catalog.
- **A default provider on the fact type** (`DefaultProvider`, with
  `requires_with_default::<F>()` next to `requires::<F>()`), adding itself to a
  `PhasePlugins`. Made a core fact type name a concrete plugin and know the
  composite, could not express "nothing may be added to this slot", and left a
  third-party plugin unfindable. Superseded by the catalog.
- **The requiring plugin naming the default** (`requires_or(..)`). Two plugins
  could suggest different defaults for the same fact with nothing to decide
  which wins. Rejected; a requirement names a fact and nothing else.
- **Adding a provider whatever it brings along**, checking it like every other
  plugin afterwards (the engine's previous resolution). A provider that does not
  fit, or requires what nothing provides, turns one missing fact into a problem
  with a plugin the operator never chose and cannot see in what they
  configured, and a UI could not tell an issue an addition fixes from one it
  replaces. Superseded by adding the implied plugins all or nothing.
- **Resolving inside `AddPhase::apply`.** Replay would then depend on which
  plugins are registered, so linking a crate in would change every saved log.
  Rejected.
- **Sorting in `AddPhase::new` and rejecting an unsorted list on load** (the
  previous design). Left a `PhasePlugins` that was not in running order to
  exist, and every way of building one had to remember the sort. Superseded by
  the invariant.
- **Sorting in the engine**, in `PhasePlugins::new` and a `deserialize_with` on
  its list of calculators. Kept the invariant, but in the one composite that
  knew about it rather than in the type of the list. Superseded by
  `DependencyOrderedSet`.
- **A typed builder for `PhasePlugins`** (`PhasePlugins::new(..).rank_by(..)
  .calculate(..)`), and turning built plugins back into a draft
  (`Draft::from_built`) to add the implied plugins. A second way to make a phase
  that skipped them, could not leave a field unanswered, and went from typed
  plugins to configs only to build the same plugins again. Removed; plugins are
  configured through drafts alone in production.
- **Tests configuring every plugin through drafts** (the previous state, after
  the builder was removed). Every test that only needed a phase spelled out
  configs by field name and went through a draft, which is clumsy and hides what
  the test is about. Superseded by constructors behind the `test-util` feature.
- **Metrics as a kind of requirement of their own** (`required_metrics` next to
  `required`). Duplicated every rule for facts. Superseded.
- **The engine adding a calculator's metric to its declaration** from
  `default_metric_value()`. Structural, but made calculators a special case of
  collecting declarations. Superseded by calculators declaring their metric,
  checked by a test.
- **`declare` on each axis trait.** Repeated the same method per axis and left
  the engine no uniform way to ask a plugin. Superseded by `Plugin::declare`.
- **A blanket `impl<C: RankingCriterion> Plugin for C`** (the previous design),
  which made a criterion's requirement impossible to leave out. `Plugin` is a
  trait of `plugin-factory` now, and the orphan rule forbids a blanket impl of a
  foreign trait. Superseded by `requires_metric`.
- **A plugin replacing a fact of the context** (the previous design), e.g. a
  draw pairing entrants up providing `EntrantsPerRanking(2)` over the
  competition's `EntrantsPerRanking(1)`. `plugin-factory` treats it as a fact
  provided twice. To be revisited when a phase's context comes from the phase
  before it.
- **Criteria that are no plugins**, held by one `LexicographicalCriteria` slice
  ranker that declared their metrics by running each criterion against a
  recording `RankingOrder` (the previous design). Kept criteria out of the
  axes, but needed a second, closed mechanism to state an order and a second
  trait object with tags of its own. Superseded by criteria being slice rankers.
- **A `Ranker` axis above `SliceRanker`.** No ranking that was foreseen is not
  a chain of slice rankers, so it was a layer of trait objects without a second
  implementation. Removed; a phase is always ranked by a `HierarchicalRanker`.
- **A boxed plugin as the default provider**, put into its slot by the
  resolution. `Box<dyn Plugin>` cannot become a `Box<dyn MetricsCalculator>`
  without the trait-to-trait casts `intertrait` existed for, and an enum over
  the axes makes the resolution axis-aware. Superseded by a registry per axis,
  which hands out a `Box<K::Object>`.
- **Optional accessor on the base trait** (`fn as_xyz(&self) -> Option<&dyn
  XYZ>`). Only core can add such accessors. Rejected as the general mechanism;
  still fine as a local pattern.

## The plugin catalog

*Implemented by `plugin-factory`. The CLI configures phases through a generic
questionnaire driven by the schemas and a `Draft`; `add-phase <FORMAT>` looks
the format up in the catalog and answers every other question as offered, i.e.
as the format recommends.*

Every consumer that configures a phase has to know every plugin: the CLI holds a
table of labels, descriptions and default parameters, and a plugin of another
crate appears in none of them. The engine needs the same knowledge to fill a gap
a requirement leaves.

### Decision

**A plugin registers itself, described.** A plugin derives `Plugin`, which
states its id, label and description and reads its schema — the questions a UI
asks to configure it, with the defaults it offers — from its fields and their
`serde` attributes. `#[register]` on its impl of the axis trait submits its
entry to the catalog of the axis, collected with `inventory`, so the catalog is
whatever is linked in, plugins of other crates included, and nothing enumerates
them:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Plugin)]
#[plugin(id = "tournament_engine::points", label = "points",
         description = "…", declare_static = Self::declare_static)]
pub struct PointsCalculator {
    #[plugin(label = "points for a win", default = Score::ONE)]
    points_for_win: Score,
    // …
}

#[register]
impl MetricsCalculator for PointsCalculator { /* … */ }
```

**One registry per axis, typed.** `#[axis]` generates the registry of an axis
next to its trait, and a plugin is built as a plugin of its axis, so a consumer
gets a `Box<dyn MetricsCalculator>`, never something to downcast.

**What may be added is the composite's shape**: `Draft::add_implied` only appends to
the fields of `PhasePlugins` taking a list of plugins, so it never adds a
second phase format.

**The ids are a contract.** A plugin's id is its `typetag` tag, so it names the
plugin in a saved log and snapshot as much as in a saved configuration. Ids are
namespaced with the crate the plugin lives in (`tournament_engine::points`) and
pinned in a test.

**Defaults are offered, never filled in.** A field's `#[plugin(default ..)]` is
what a UI offers as its answer, so the engine's plugins carry the defaults its
catalog used to create them with (the points of chess, the classical
Sonneborn-Berger weights). Building a plugin still needs every field answered.
The offered answer is separate from a `#[serde(default)]`, which only says what
a missing field is read as when loading: offering an answer must not make a
saved plugin that lacks the field load silently, and reading a field leniently
must not suggest that value to an operator.

**UI text lives on the plugin**, which means the engine ships English;
localisation would wrap it, keyed by the plugin id.

### Alternatives

- **One global registry** with a `build` returning `Box<dyn Any>`. One
  `collect!` for everything instead of a generated type per axis, but trades a
  compile-time guarantee for a downcast. Rejected.
- **Generating the CLI's enums** from the plugin list. A consumer-side table
  either way, and no third-party plugin ever appears in it. Rejected.
- **A default provider per fact type**, see
  [Compatibility between plugins](#compatibility-between-plugins). Superseded.
- **A hand-written registration per plugin** (`plugin_kind!` and
  `inventory::submit!` of a `PluginEntry` with an optional constructor, the
  previous design). Strings repeated next to the type, and no schema. Superseded
  by the derive.
- **`Accepts<K>` impls stating which axes a composite may be added to** (the
  previous design). Superseded by the shape of the composite.
- **A field's `serde` default as the answer it offers** (the previous design).
  One attribute for two concerns: offering a default made the field optional
  when loading, and a field read leniently was suggested to operators.
  Superseded by `#[plugin(default ..)]`.

## Metrics: plugin-produced ranking data

A `Ranking` carries the data used to sort it — points, wins/draws/losses,
Buchholz, Sonneborn-Berger, round of first loss, and so on. Which of these exist
varies per phase, and third parties must be able to add new ones.

### Decision

**Metrics are identified by Rust type.** A metric is any type implementing
`TypeMapValue`; `Metric = Box<dyn TypeMapValue>` and `MetricId = TypeId`.

`MetricId` stays a `TypeId` deliberately, distinct from the `typetag` identity
used to serialize a metric: disjointness and requirement checks run fresh
in-process on live plugin values, never on serialized data, which is exactly
where `TypeId` is stable and free.

**`Ranking` stores its metrics in a `TypeMap`** — an `IdMap<Box<dyn
TypeMapValue>>` whose id is the concrete type's `TypeId`, so it serializes as a
plain sequence, since each value carries its own `typetag` tag. `Ranking::metric::<T>()` resolves to `Option<&T>` via
`TypeMap::get`, which uses `downcast_unchecked_ref` because the key already
proves the concrete type. Keyed by type rather than a `Vec` scan, so a ranker
that reads several metrics per comparison pays a hash lookup, not a linear
search.

**`MetricsCalculator` computes incrementally**, rather than recomputing a whole
ranking from scratch:

```rust
#[axis]
pub trait MetricsCalculator: Plugin + DynClone {
    fn default_metric_value(&self) -> Metric;
    fn update_after_match_result(
        &mut self, phase_data: &PhaseData, r#match: &Match,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)>;
    fn update_after_bye(
        &mut self, phase_data: &PhaseData, bye: &Bye,
        old_metrics: &OldMetrics,
    ) -> Vec<(RankingId, Metric)>;
}
```

(`update_after_bye` is explained in [Byes](#byes).)

`default_metric_value()` both seeds every ranking at phase start and declares
which metric type this calculator owns, so disjointness is checkable without
running any computation. `old_metrics` lets a calculator that depends on another
calculator's output (e.g. Buchholz on points) read the pre-update values of the
calculators that ran before it, typed through `OldMetrics::of::<M>()`, while
`PhaseData` holds their new ones. Both are
only complete if those calculators did run before it, so calculator order does
matter; it is derived from their declared dependencies, see
[Compatibility between plugins](#compatibility-between-plugins).

**The matches a ranking played are core data, not a metric.**
`Ranking::played_matches` lists them in report order, appended in place before
the calculators run. It used to be a `Matches` metric of its own calculator,
which every opponent-based metric (Buchholz, Sonneborn-Berger) had to require,
and which cloned the whole list on every reported result. A calculator does not
need `MatchResult` either: a match is reported once, so a reported match was
always unfinished before, and the update takes no previous result.

**Ranking comparison stays typed.** The `HierarchicalRanker` applies a list of
`SliceRanker`s, each resolving ties left by the previous. `rank_by`, which every
criterion sorts with, looks up its metric once per ranking of the slice and
sorts the rankings together with the typed values, so a comparison neither
looks up nor downcasts anything. Each criterion only sees the slices still tied,
which is usually a small part of the phase after the first one. Grouping all
criteria into one sort over pre-extracted columns per metric (the previous
`LexicographicalCriteria`) was measured to be about 20% slower on the
`large_tournament` benchmark, since it extracted every metric for every ranking
and compared through a trait object per column.

The active calculators' metric types being pairwise disjoint, and metrics
required by other plugins being present, are checked as facts once per phase,
see [Compatibility between plugins](#compatibility-between-plugins).

### Alternatives

- **String-keyed metrics.** Untyped bag. Rejected.
- **A dedicated `RankData` trait with `cmp_for_leaderboard`** (an earlier draft
  of this design). Gave core one fixed comparison per ranking type; the
  `SliceRanker` chain gives composable tiebreaking instead, and `TypeMap` already
  supplies typed access. Superseded.
- **Metrics as marker traits.** A marker trait answers "does this instance
  implement behavior X", not "what data did calculating for this ranking
  produce", and offers no way to express disjointness. Rejected as a different
  concern.
- **Runtime-discovered produced metrics** (inspect what `calculate` returned).
  Would require running calculation just to validate configuration. Rejected in
  favor of the static declaration.
- **Recomputing all metrics per result** instead of incrementally. Simpler
  calculators, but cost grows with tournament size on the hottest path.
  Rejected.

## Scores

Points, and the tiebreaks summing or weighting them (Buchholz,
Sonneborn-Berger), are compared, sorted and hashed, and have to be the same
after a replay on every platform.

### Decision

**They are `Score`, a finite `f64` with a total order.** It is never NaN or
infinite, and `-0.0` is made `0.0`, so `==`, `Ord` (`total_cmp`) and `Hash`
(the bits) agree, and `Eq`, `Ord` and `Hash` hold. An operation whose result is
not finite panics. It is shown, parsed and serialized as its `f64`, and a plugin
field of it takes any decimal answer.

- **Deterministic everywhere.** `+`, `-` and `*` on `f64` are exactly rounded
  by IEEE 754, so the same calculation gives the same bits on every platform,
  natively and in wasm; Rust neither enables fast-math nor fuses operations.
  Only functions like `exp` or `ln` differ between platforms, which a score
  does not use.
- **Exact for what is played.** Every scoring system in use counts in halves or
  quarters, which an `f64` holds exactly, and so are their sums and the classic
  Sonneborn-Berger products (weights 1 and 0.5). Only a value with no exact
  binary form, such as `0.1`, can leave two mathematically equal scores
  unequal, depending on the order of additions. A field that must rule that out
  narrows itself with `#[plugin(multiple_of = 0.25)]`, see [Ranges of
  numbers](#ranges-of-numbers).
- **The wrapper is needed anyway.** `FieldType` is foreign to the engine and so
  is any float type, so no crate's ordered float could be a field without one.

### Alternatives

- **Plain `f32`/`f64`** (the first design). No `Eq`, `Ord` or `Hash`, NaN
  possible, and `f32` loses precision on large sums. Rejected.
- **`ordered_float::OrderedFloat`.** Orders NaN rather than ruling it out, and
  still needs a wrapper for `FieldType`. Rejected.
- **An `i32` counting quarters** (the design before this one). Exact by its
  type for every sum, but it needs its own parsing, display and serialization,
  rounds products, and refuses anything finer than a quarter, for exactness no
  scoring system in use needs. Rejected as too much for the gain.
- **`rust_decimal`.** Exact for every decimal anyone types, but 16 bytes per
  value and a heavier comparison. Rejected.
- **`fixed::FixedI32<U2>`.** Its `Display` shows the shortest text that tells
  quarters apart (`66.8` for 66.75), its parsing rounds `0.1` to `0` silently,
  and its serde form is `{"bits": 2}`. Rejected.
- **Rationals** (`num-rational`). Exact even for thirds, but every comparison
  cross-multiplies and every sum normalizes by a gcd, on the hot path. Rejected.

## Ranges of numbers

A field taking a number states which numbers in the schema, so that a UI can
show and check them and an operator learns that a number is no answer before
anything is built.

### Decision

- **`FieldKind::Integer(IntegerRange)`** has a `min` and a `max` always (an
  integer type's own bounds) and a `multiple_of`, 1 unless narrowed. A bound
  is *stated* if chosen for what the number means (by a field attribute or
  `IntegerRange::new`) and only a *limit* otherwise (`IntegerRange::limits`,
  e.g. `u32::MAX`). Both bound the range, and a UI may encode both, e.g. as the
  bounds of an input; the description an operator reads names only stated
  ones.
  **`FieldKind::Decimal(DecimalRange)`** has each of them optionally; `f64`
  has none.
- **`multiple_of` is anchored at 0**, as JSON Schema's `multipleOf`: the numbers
  are the multiples of it from `min` to `max`, and `min` and `max` have to be
  multiples themselves, so that "from `min` in steps of `multiple_of`" means the
  same. For a decimal it is a whole number or a power of two, whose multiples
  `%` on floats checks exactly.
- **The type declares, a field narrows.** `FieldType::kind` gives the type's
  range (`u8`: 0 to 255); `#[plugin(min, max, multiple_of)]` on a
  field narrows it, never widens it. A narrowing the type does not allow (a
  bound that is no multiple, a `multiple_of` that is no multiple of the type's)
  or a default out of the range panics when the schema is built, since the
  type's range is only known then.
- **Checked twice, by the schema and by the type.** `FieldKind::walk` rejects a
  number out of range with `ConfigError::OutOfRange` naming the field, and a
  prompter asks for numbers of the range only. The type's own `Deserialize`
  still checks, since a snapshot or log is deserialized without a schema.

### Alternatives

- **A step anchored at `min`** (HTML's `step`). Makes the numbers depend on two
  declarations that may come from the type and the field (`min = 0.1` on
  multiples of 0.25 leaves no number) and cannot be checked exactly for
  decimals; a setting that needs odd numbers is rephrased instead (best of N as
  first to K wins). Rejected.
- **Checking only in `Deserialize`.** Reports a wrong number late, as the plugin
  rejecting its answer, and tells a UI nothing. Rejected.

## Consumer data on entities

Consumers want to attach data core never reads — a table assignment, a database
id, a club.

### Decision

Consumer data lives in a `TypeMap`, and **enters through the log, at the
transition where it becomes known.** That second half is forced by determinism
under replay: a creation-time factory supplied by the consumer could only compute
data derivable from state already present, which is the one kind of data that
needs no storage at all. Everything genuinely interesting — an operator's input,
a timestamp, an external id — is not derivable, so it must arrive as transition
data.

**The `TypeMap` is a field on the entity itself**, not a side map keyed by entity
id. Consumer data is then reachable from a bare `&Entrant` or `&Match`, with no
need to carry the owning `Phase` around just to resolve it, and access reads
`r#match.annotation::<TableNumber>()`.

What differs per entity is only *which transition* writes it, which follows from
who creates the entity:

- **Entrants** are consumer-created, so the `TypeMap` on `Entrant` is populated by
  the `AddEntrant` that creates it — an `Entrant` is built with
  `Entrant::new(id).with_annotation(value)` and read with
  `entrant.annotation::<ClubId>()`.
- **Matches** are created by the phase format, so the `TypeMap` on `Match` is
  populated by the format when it schedules the match
  (`Match::new(..).with_annotation(value)`), with whatever it knows about the match
  beyond its round (see "Rounds and stages"). A consumer is not in the loop there,
  so consumer annotations (*planned*) are necessarily a second step: an `AnnotateMatch`
  transition carrying `Box<dyn TypeMapValue>` (already `typetag`-serializable, so
  the consumer's type tags itself) writes into the `TypeMap` of an existing
  `Match`. An `Option` on lookup is honest here — the annotation genuinely does
  not exist when the match is born.
- **Rankings** get nothing new. Consumer data fixed at phase start is a pure
  function of the entrants' data, so it is derived on demand; data that evolves
  with results is a metric, which already exists. If per-ranking consumer data is
  ever needed it gets its own field rather than sharing `Ranking::metrics`, since
  a metric type is owned by the calculator that produces it.

The memory cost is accepted deliberately: `Match` is 72 bytes and `TypeMap` is 56,
so every match grows by roughly 78% whether or not anything annotates it. An empty
`TypeMap` does not allocate, so the cost is flat bytes rather than an allocation
per match, and it buys an entity that is self-contained.

Undo needs no inverse: it is a replay without that log entry, and re-annotating
the same type overwrites via `TypeMap::insert_boxed`.

**Personal data is the exception: it stays out of the log.** A name, a contact,
a birth date has to be deletable, and a log replays as it was written and cannot
have parts removed. So a consumer keeps personal data in a store of its own keyed
by `EntrantId` (the CLI's `People`), saved next to the log in the same file, and
the log holds only the ids. That store keeps everyone ever entered, removed
entrants included, since an undo or redo can bring them back. This is the
side-table rejected below for consumer data in general; for personal data,
being deletable outweighs sharing the persistence paths, and nothing in the
engine reads it.

This requires `TypeMapValue: DynClone`, because `Transition::apply` takes
`&self` and so must clone its payload on every replay. That ripples into
`Metric`, but every metric type is a trivially-`Clone` plain struct, and it
mirrors the `DynClone` supertrait of every plugin axis. It also costs `Entrant` its `PartialEq`
derive; comparison becomes field-wise, as already done for `Ranking` in the
serialization tests.

### Alternatives

- **A sparse `Map<MatchId, TypeMap>` on `Phase`** instead of a field on `Match`.
  Costs nothing for consumers that never annotate, and only allocates for matches
  that are annotated. Rejected: it makes consumer data unreachable from a bare
  `&Match`, forcing every display path to carry the owning `Phase`, and the flat
  per-match bytes are not a real constraint at this crate's scale.
- **Consumer-owned side-tables keyed by entity id** (the previous decision here).
  Zero core change, but a side-table shares neither of the persistence paths: it
  is not in the snapshot and not in the log, so its data must be persisted
  separately, kept from drifting, and is untouched by undo. Superseded once both
  persistence paths became requirements, except for personal data (see above).
- **A core `name: String` field on `Entrant`.** Concedes that a display label is
  universal, but gives core no principle for refusing the next field — country,
  club, seed rating, external id. Rejected.
- **A consumer-supplied factory called at entity creation.** Restricted by
  replay purity to derivable data, as argued above. Rejected.
- **Generic entities.** See below.

## Why entities are not generic

An obvious alternative to the above is to make the entities themselves generic —
`Competition<E, M, R>` with the library working through traits such as
`AsRef<Entrant>`/`AsMut<Entrant>`, plus a trait method to construct a match from
current competition state. Recorded here because it is the natural idea and it
fails for reasons that are easy to rediscover.

Three independent blockers, all verified against `typetag` 0.2.23's source:

1. **Generic entities force generic plugin traits.** Every plugin method takes
   `&PhaseData`, so `PhaseData<M, R>` makes `PhaseFormat<M, R>` generic.
   `typetag-impl/src/tagged_trait.rs:7` rejects generic traits in `de` mode
   ("deserialization of generic traits is not supported yet"), so
   `Box<dyn PhaseFormat>` would lose `Deserialize` and the snapshot path would
   have to be hand-rolled.
2. **Reusable third-party plugins need generic impls.**
   `impl<M: MatchLike> PhaseFormat<M> for RoundRobin` hits
   `typetag-impl/src/tagged_impl.rs:7`, which rejects generic impls in `de` mode
   for the same reason.
3. **The log needs concrete transitions.** Serializing the log means tagging
   transitions, so a generic `AddEntrant<X>` carrying `X::EntrantData` cannot be
   registered either. Core could then ship no loggable transitions at all; each
   consumer would have to define its own registry and newtype-wrap every core
   transition.

A narrower variant — one `Extension` parameter with associated data types per
entity, keeping core entities concrete so plugin traits stay non-generic —
avoids blockers 1 and 2 but not 3. It also turns out to buy little: its one real
advantage over `TypeMap` is non-optional typed access, and for match and ranking
data the `Option` is semantically correct anyway (see above), so two of the three
associated types would want to be `()`. A single consumer struct per entity also
cannot compose: a CLI, a web layer and a third-party scheduler could not annotate
independently without agreeing on one type, which `TypeMap` gives for free.

Rejected. Generics would only ever decide the *type* of consumer data, never how
it arrives — and how it arrives is the actual constraint.

## Core-owned identity

Entity ids are deterministic so that replay reproduces them: `MatchId` and
`RankingId` are UUIDv5 values derived from content (the participating ranking
ids, plus the round for a match) under a per-entity namespace constant. A match
is therefore identified by who plays it, not by when it was created.

UUIDs are not usable for human input, so a match also needs a small number.
*Planned:*

- **Derived, not stored.** `Phase::matches` is an `IdMap` wrapping an
  `IndexMap` in insertion order, and matches are only ever inserted, so the
  number is the insertion index plus one. Lookup both ways is O(1)
  (`get_index_of` / `get_index`), nothing new is serialized, and uniqueness is a
  structural property of the map rather than something an assignment site has to
  get right. `map.rs` already establishes that deriving state from iteration
  order is sound.
- **Core assigns, plugins never.** `Match::new` is called by third-party
  `PhaseFormat`s and keeps its current signature, so a plugin has no way to
  fabricate or duplicate a number.
- **Per-phase scope**, since `Phase::create_new_matches` has no access to the
  `Competition` and multi-phase competitions are not supported yet.
- **Positional, so not durable.** Undoing an earlier transition renumbers later
  matches. `MatchId` is the stable identity; a match number is for display and
  input only, and should be documented as such.

### Alternatives

- **A stored field set by a `pub(crate)` setter during insertion.** Smallest
  diff, but the invariant is upheld by one call site rather than by the types.
  Rejected while the derived form is available.
- **Splitting `NewMatch` (what a format returns: teams and round) from `Match`
  (that plus id and number).** The genuinely compiler-enforced version, and it
  would also retire the `debug_assert` on match id uniqueness. Deferred: worth
  doing if matches ever become removable or reorderable, which is what would
  break the derived form.
- **A number supplied by the phase format.** A self-reported fact a plugin could
  get wrong. Rejected.

## Rounds and stages

Knockout rounds have names — round of 16, quarterfinal, final, match for third
place — and a double elimination adds a winner and a loser bracket, whose rounds
are named per bracket. The question is whether the core's `Match::round` should
carry that structure.

### Decision

**A round is a scheduling slot, nothing more.** `Match::round` is a
`NonZeroU32` with a contract: a ranking plays at most once per round, and a
ranking's matches in later rounds are played after those in earlier ones. The
core relies on exactly that and on nothing else:

- the check that no ranking plays twice in a round;
- `MatchId`, which tells a rematch of the same pair apart by its round (in a
  double elimination two entrants can meet in both brackets, and again in a
  bracket reset);
- metrics that read a later round as a later match, like `RoundOfFirstLoss`,
  where knocked-out entrants are ranked by when they were knocked out.

**The structure is a typed annotation written by the format.** The format is the
only party that knows which stage a match belongs to, and the round number alone
does not tell: the final and the match for third place share a round, and in a
double elimination a winner-bracket round runs alongside a loser-bracket round.
So the format attaches a value of its own type when scheduling the match, e.g.
`KnockoutRound::RoundOf(8)` or `KnockoutRound::ThirdPlace` for
`SingleElimination`, and a consumer maps that to a name in its own language.
Formats without stages (round robin, Swiss) attach nothing, and the round number
is all there is to show.

### Alternatives

- **Names as strings in the core.** Display data, language-dependent, and an
  untyped bag in disguise. Rejected.
- **A core enum of round kinds** (`Winners(n)`, `Losers(n)`, `Final`, ...). A
  closed vocabulary third-party formats cannot extend, and meaningless for round
  robin or Swiss. Rejected.
- **A format-defined round type (`Box<dyn ...>`) instead of the integer.** Every
  use above would need to order values of unrelated types, and comparisons on the
  result-reporting hot path would become virtual calls. Rejected.
- **The consumer translating the round number alone.** Cannot distinguish stages
  that share a round, as shown above. Rejected.

## Byes

A ranking that has no match in a round, because the field is odd, a bracket is
not full or it sits a Monster DYP round out, often scores for it anyway, e.g. a
full point in a Swiss. A calculator cannot tell that from the matches: a ranking
missing from a round may have a bye, but it may also have been knocked out or not
be scheduled yet, and only the format knows which.

### Decision

**A bye is an entity of its own, scheduled by the format.** `Bye` holds its
ranking, its round and annotations, like a `Match` without an opponent and
without a result. Its `ByeId` is a UUIDv5 of ranking and round. The format
returns byes next to matches in a `Schedule { matches, byes }`, and the phase
stores them in `PhaseData::byes`, apart from the matches, so match numbers stay
the positions of matches alone. The round contract covers both: a ranking is in
at most one match or bye per round, and the bracket formats move the annotations
of a pairing decided without a match onto its bye, so a consumer can name its
stage.

**A bye is final when it is scheduled.** A match is created open, gets a result
later and may get another one; a bye has nothing to report and never changes.
So it reaches calculators through a hook of its own,
`MetricsCalculator::update_after_bye`, run right when the format schedules the
bye, followed by a re-rank. The hook has no default implementation: whether a bye
counts is a different question per metric (points: configurable; matches played,
round of first loss, round of elimination: no; Buchholz and Sonneborn-Berger: no
opponent to count, but the points of the bye count for the ranking's
opponents), and the compiler makes every calculator answer it.

**Scheduling a bye does not schedule again.** Only a reported result calls the
format, even though a bye can change the ranks the format pairs by. The format
knows about every bye it returns when it returns it, so a second call would only
see what it already decided.

**Points stay calculated from counts.** `WinsDrawsLosses` counts byes next to
wins, draws and losses (but not as matches played), and `PointsCalculator`
scores them with `points_for_bye`, 0 unless set.

*Not covered yet:* kinds of byes worth different points (a requested half-point
bye, a sit-out scored with average points). A requested bye is transition data
the format has to accept, so it is a feature of its own, and the kind would then
need a vocabulary calculators can read. Forfeits are not byes: they have an
opponent and are a match result.

### Alternatives

- **One enum `Match | Bye` in and out**, i.e. formats emit it and calculators
  receive it in place of the match. Calculators would need a `Bye` arm in
  `update_after_match_result` that can never run, since nobody reports a
  result for a bye, and one collection of both would mix byes into the match
  numbers. Rejected; the format's side of it survives as `Schedule`.
- **A bye as a degenerate match** (no opponent, or a preset result). Every
  calculator would see byes through the existing path and silently get them
  wrong: Buchholz looking up an opponent, the played matches counting it. Every
  consumer of `Match` would have to handle a match without opponent. Rejected.
- **A phantom "bye" ranking** paired like any other and auto-reported as beaten.
  Breaks the contract that a format only pairs rankings of its phase, and the
  phantom would get metrics, a rank and feed into Buchholz. Rejected.
- **Calculators inferring byes from gaps in the rounds.** Cannot tell a bye from
  an elimination or a match not scheduled yet. Rejected.
- **The format stating what a bye is worth.** Points and their scale are the
  points calculator's configuration; the format would reach into another
  plugin's concern. Rejected.
- **A default no-op `update_after_bye`.** Less to write per calculator, but a
  calculator that forgets byes would be silently wrong instead of failing to
  compile. Rejected.

## Persistence

### Snapshots

`Competition` and everything in it — `Phase`, `Ranking`, every embedded plugin
and metric — is serializable with [`typetag`](https://docs.rs/typetag). The trait
opts in once and each impl should carry a short, explicitly chosen, namespaced
tag. For a plugin axis `#[axis]` and `#[register]` do both, the tag being the
plugin's id; other traits name it themselves:

```rust
#[typetag::serde(tag = "type", content = "value")]
pub trait TypeMapValue: Any + Debug + DynClone {}

#[typetag::serde(name = "tournament_engine::eliminates")]
impl TypeMapValue for Eliminates {}
```

Metrics, annotations, plugins and every `CompetitionTransition` impl name their
tags. A metric's tag may equal the id of the plugin calculating it (both
`tournament_engine::points`), since every trait object has a registry of its
own.

This gives `Box<dyn Trait>` a working `Serialize`/`Deserialize` backed by an
`inventory`-populated tag → type registry inside `typetag`, with no registry
code here. Two findings verified against `typetag`'s source rather than its docs:

- **Adjacent tagging is required for non-self-describing formats** such as
  postcard, which is why every annotation above carries a `content`. Internal
  tagging serializes the content through an adapter that reports
  `is_human_readable() == true` whatever the real format says, while
  deserialization sees the real format — so a `Uuid` is written as a string and
  read back as bytes, and fails. Adjacent tagging keeps the content out of that
  adapter. Cost in JSON is one nesting level; it also halves the postcard payload
  versus giving ids a format-independent string form. Upstream bug, reported
  against `typetag` 0.2.23.
- **A tag collision fails loudly.** The registry is
  `BTreeMap<&'static str, Option<DeserializeFn<T>>>`; a second registration of
  the same tag overwrites the entry with `None`, and deserializing it errors
  ("non-unique tag of ...") instead of picking either impl.

`IdMap` and `TypeMap` both serialize as a plain sequence of values with their
keys omitted, since every value carries its own key — an id for `IdMap`, a
`typetag` tag for `TypeMap`. This is sound only because both wrap an `IndexMap`,
so serializing walks insertion order and deserializing reinserts in that order.

The one thing `typetag` does not give is a fixed-size binary discriminant; tags
are strings, so wire cost scales with tag length. Mitigated by keeping tags
short, and acceptable for occasional persistence.

### The transition log

`Transition<S>` is generic over the state, so it cannot be `typetag`-annotated
upstream. Serializing the log therefore needs a non-generic sub-trait here:

```rust
#[typetag::serde(tag = "type", content = "value")]
pub trait CompetitionTransition: Transition<Competition> + Debug {}
```

with the machine becoming
`CompetitionStateMachine = StateMachine<Competition, Box<dyn CompetitionTransition>>`
— which `undoable-state-machine` anticipates via its blanket impl for
`Box<dyn Transition<S>>`, so the box needs nothing further to be applied. The
trait adds no methods: implementing it is purely the statement that a transition
is part of the persisted log, which is also what makes it the one place a
consumer's own transition opts in. Every implementor needs
`Serialize`/`Deserialize` and an explicit tag, and all six core transitions have
both.

That composes into the whole machine: with `undoable-state-machine`'s `serde`
feature its log types are serializable whenever the transition type is, so
`CompetitionStateMachine` serializes as its log — undo entries included — and
nothing in this crate has to describe a log entry itself.

Deserializing rebuilds the machine through `StateMachine::build`, which replays
the log rather than trusting a stored `current`. Two consequences worth naming:
the current state is never written, so it cannot drift from the log that produced
it; and a log that no longer replays cleanly fails to deserialize instead of
loading into a state no sequence of transitions could reach. Replay is already a
hard requirement for undo, so this costs no determinism that was not owed
anyway — it just also makes it a load-time check.

This is what separates the two persistence paths in practice. A snapshot is
smaller and loads without replaying, but holds only the current state; a log also
holds how that state came about, so a change made before a restore, such as a
reported result, can still be undone after it. What a session can undo by
`undo`, though, is only what it did itself, see
[Undo and redo of a session](#undo-and-redo-of-a-session).

### Tag stability

A tag is a persistence contract, not a name. Renaming or moving a type must not
change it, which is why tags are written out explicitly rather than derived. This
spans plugins, metrics, consumer annotations and transitions — so it is a single
cross-cutting commitment, not a per-mechanism detail. Each mechanism pins its
tags in a test, so a rename fails there instead of silently invalidating old
data.

Rejected alternatives for tagging: **`std::any::TypeId`**, free and
collision-proof but only stable within one build, so it cannot survive a
snapshot; **a hand-rolled `TypeTag([u8; 8])` + `erased_serde` + `inventory`
registry**, a smaller discriminant but a reimplementation of what `typetag`
already does, with less testing; **an auto-derived tag** (a hash of the type
path), which implementors cannot forget but which silently changes when a type is
renamed or moved — the same drifting-fact failure rejected elsewhere in this
document.

## Progression as an undoable state machine

Tournament progress — registrations, reported results, round advances — is a
sequence of transitions applied to an initial state, so transitions can be
undone. The mechanics are generic and live in the separate
`undoable-state-machine` crate; this crate instantiates it with `Competition` as
the state, through the `CompetitionStateMachine` alias, and enables that crate's
`serde` feature so the log it keeps is persistable.

Undo is implemented there as a replay of the still-active log entries from the
initial state, which is what makes determinism under replay a global constraint
rather than a local concern: it is why ids are content-derived, why consumer data
arrives as transition data, why a match number is positional rather than
durable, and why plugin state needs no undo of its own.

`Transition` keeps `check_applicable` and `apply` separate, and `Competition`'s
mutators stay crate-internal, so state can only change through a logged
transition.

### Undoing a change by what it did

The state machine undoes a `TransitionId`, which a consumer only has if it kept
the ids it applied. Undoing a reported result that way means walking the log back
through every change made since, which is not what an operator correcting a
result reported a while ago wants. `CompetitionStateMachineExt::undo_match_result`
takes the phase and the match instead, finds the latest log entry still in effect
that reported that match, and undoes that one. It lives in the engine, not in the
consumer, because finding the entry means knowing the shape of the log.

- **Found by downcasting**, which is what `Any` as a supertrait of
  `CompetitionTransition` is for. A transition in a log is a
  `Box<dyn CompetitionTransition>`, so recognizing a `ReportMatchResult` among
  the entries needs either this or a trait method every transition answers for
  itself — i.e. a self-reported fact a third-party transition could get wrong.
  The cost is that only transitions this crate ships can be found, so a
  third-party transition that sets a result is reported as not undoable rather
  than silently mishandled.
- **Rejected, not cascaded, when a later change depends on it.** Undoing the
  semifinal of a knockout whose final has a result would leave that report
  without a match, which the state machine surfaces as the failing entry and the
  undo is refused with the log unchanged. Undoing the dependent reports as well
  would be a second, implicit decision about what the operator meant.
- **No new transition.** An undo entry is what the log already has for reverting
  a change, and reusing it keeps the undo itself undoable.

### Changing a phase after it was added

A consumer changing a phase reads its config from the current state
(`Phase::config`), puts it into a draft (`Draft::with_answers`), lets the
operator change it as when adding the phase, and hands the plugins built from it
to `CompetitionStateMachineExt::reconfigure_phase`. That appends a
`LogEntryKind::Replace` of the `AddPhase` in effect by one adding the same phase
with the new plugins, unless their config is the phase's already. The phase is
then played as if it had been added that way, at any point of the competition.

- **A replacement, not a transition of its own.** The replacement runs in the
  slot of the `AddPhase`, so everything after it replays on the changed phase:
  the reported results stay as long as the changed phase schedules their
  matches, and the metrics and ranks are recalculated rather than patched. A
  change a later entry does not fit is rejected by the state machine, with the
  log unchanged, just like an undo a later entry depends on. A transition
  changing the phase in place would have to migrate the phase's state, i.e.
  every plugin would need to know how to take over from another configuration.
- **The config is serialized from the state, not stored.** A plugin serializes
  as the answers it is configured with, plus its runtime state in fields its
  schema leaves out, which `Config::of` drops by checking the serialized form
  against the schema. So the config needs no copy of its own, neither in the
  log nor in the phase, and a snapshot can be reconfigured as well as a log. It
  relies on every plugin serializing as it deserializes, which loading a log
  relies on already. Rejected: `AddPhase` holding the config and building the
  plugins whenever it is applied (built on every replay, twice, and the config
  had to be persisted in non-self-describing formats, which cannot read a
  `serde_json` value back). This turns built plugins back into answers, unlike
  the `Draft::from_built` rejected above for adding the implied plugins: the
  implied plugins are answers by now, so an operator removing what required one
  removes it too.
- **Found through the phase id.** The replacement keeps the phase id, so the
  `AddPhase` in effect that added a phase is found by downcasting, as a report
  is for `undo_match_result`.
- **Undone like any action.** The replacement is an action of the session that
  made it, see below, so undoing right after a change of the phase reverts that
  change.

### Undo and redo of a session

`undo` and `redo` work as in an editor: they undo and redo what the operator
did, in reverse order, not what the log happens to end with.
`undoable_state_machine::SessionHistory` keeps the ids of the entries a session
appended as its actions — every change it made, including an undo of an entry
of its choice such as `undo-report`, and a reconfiguration. `undo_last` undoes
the latest of them that is in effect, `redo_last` redoes what it undid last as
long as the session took no action since, each by appending an ordinary undo
entry naming its target. The engine's operations that append an action
(`apply_in`, `undo_match_result`, `reconfigure_phase`) take the history.

- **Per session, kept outside the log.** Whose an entry is matters only for
  undo: when a log is shared, e.g. synced between a client and a server, an
  operator expects their `undo` to revert what they did, not what was pulled in
  from elsewhere, and not what they did in another session. Only the session
  knows which entries it appended and whether as an action or by `undo`, so it
  keeps that itself, and the log stays as it was. A session ends with its
  history: a loaded competition has nothing to undo, as in other editors. A
  single result can still be undone, since `undo_match_result` finds it in the
  log.
- **A targeted undo is an action.** After `undo-report`, `undo` brings the
  result back rather than undoing the change before it, which an operator
  correcting a result and changing their mind expects.
- **Explicit targets.** The entry `undo` appends names the action it undoes, so
  it means the same however the log is merged or reordered later.
- **Fails only because of other sessions.** Undoing a session's actions in
  reverse order returns to states the log was in before, so it cannot fail while
  only that session writes. A change another session made since may depend on
  the action, e.g. a result reported for a match only a reconfiguration
  schedules; then the undo is rejected like any other, with the log unchanged.
  Another session may also have undone the action itself (or, for a redo, the
  undo of it). Then the history no longer matches the log, and `undo` fails
  instead of passing over the action: passing over it would undo older actions
  while the newer one could come back, should the other session take its undo
  back. Either way the action stays the next one to undo. An action the session
  undid itself, by a targeted undo, never blocks: it lies below that undo, which
  is undone first and brings it back.
- **The seed is no action.** It is applied outside any history, so it is never
  undone, and nothing before it either.

Rejected:
- **Deriving the history from the log**, with `undo` undoing the latest change in
  effect. After `undo-report`, `undo` skipped it and undid an unrelated change;
  undoing a reconfiguration could fail even within a single session once an
  undo reached behind it; and an `undo-report` of the latest change could not be
  told apart from `undo`.
- **Dedicated log entries for `undo` and `redo`**, telling them apart from
  targeted undos in the log. They record whether an undo was navigation, but not
  whose it is, which a shared log needs, and they change the log format for
  something only the session uses.
- **An origin per user in the log.** A user with two sessions would undo in one
  what they did in the other.
- **Persisting the history**, e.g. next to the log in the CLI's file. It is not
  part of the competition, and editors do not keep it either.

## Dependencies this implies

- `typetag` (plus its internal `inventory` and `erased_serde`) and `serde` for
  serializing trait objects; `postcard` and `serde_json` exercise both a
  non-self-describing and a self-describing format in tests.
- `plugin-factory` (with `inventory`) for the plugin axes, their catalog,
  schemas, drafts and the compatibility of plugins.
- `dyn-clone` for cloning boxed plugins and boxed `TypeMapValue`s.
- `indexmap` for deterministic iteration order, with `rustc-hash` as a faster
  hasher for small `Copy` id keys.
- `uuid` (v5) for content-derived ids; `rand` and `rand_seeder` for seeded,
  reproducible shuffling; `vec1` for non-empty collections.
- No `enum_dispatch`, `linkme`, `ambassador` or `intertrait`: each was only
  needed for an alternative ruled out above.
