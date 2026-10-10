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

Every axis is its own public, unsealed trait, selected as `Box<dyn Trait>` and
constructed directly by the consumer. Implemented axes: `PhaseFormat`, `Ranker`
(with `SliceRanker`, `RankingCriterion` beneath it), `MetricsCalculator`. All
but `RankingCriterion` share the `Plugin` supertrait (`Debug + DynClone` plus
`declare`, see [Compatibility between plugins](#compatibility-between-plugins)),
and each has a hand-written `impl Clone for Box<dyn Trait>` delegating to
`dyn_clone`.

This applies uniformly to future axes: each one gets a trait and a slot, with no
per-axis judgment call. A string-keyed registry for loading a configuration from
non-Rust sources can be layered on additively later; it is not a v1 requirement.

### Alternatives

- **Closed `Kind` enum per axis.** Allows static dispatch via `enum_dispatch`,
  but third parties cannot add variants. Rejected.
- **Generic type parameters per axis.** Zero-cost, but conflicts with runtime
  selection, and every combination becomes a distinct type. Rejected.
- **Name-driven registry from the start** (`inventory`/`linkme`). Not needed to
  select an axis, since a consumer constructs the implementation it wants.
  Added later for what does need it, see
  [The plugin catalog](#the-plugin-catalog).

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
fn update_ranks(&mut self, phase_data: &PhaseData) -> Vec<(RankingId, NonZeroU32)>; // Ranker
fn update_after_match_result_changed(&mut self, phase_data: &PhaseData, /* ... */)
    -> Vec<(RankingId, Metric)>;                                           // MetricsCalculator
```

`SliceRanker::update_ranks` follows suit, since `HierarchicalRanker` calls it
with its own `&PhaseData`. Methods that only describe a plugin's configuration
(`default_metric_value`, `allows_draws`) stay `&self`.

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
`Phase` forwards the read accessors to its `PhaseData`, so consumers are
unaffected. `data` serializes as a nested field rather than with
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

## Compatibility between plugins

*Implemented: `plugins::compatibility`, `Plugin::declare`, `PhasePlugins`,
`AddPhase`, every plugin of this workspace declaring what it provides and
requires, and resolution reading the catalog (see
[The plugin catalog](#the-plugin-catalog)).*

Not every combination of plugins is valid, and detecting that must not rely on a
hardcoded list of concrete type pairs, which would be blind to third-party
implementations. The dependencies that already exist or are foreseeable run
between every pair of axes and in both directions:

- a calculator reads another calculator's metric (Points on wins/draws/losses,
  Buchholz on Points and Matches);
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

**Compatibility is checked on typed facts.** A fact is any `'static` type
describing something true of a phase, identified by its `TypeId` like a metric.
The `Plugin` supertrait every axis trait shares carries the declaration, and is
implemented by hand next to the axis trait:

```rust
pub trait Plugin: Debug + DynClone {
    fn declare(&self, declaration: &mut Declaration);
}

impl Declaration {
    pub fn provides<F: Fact>(&mut self, fact: F) -> &mut Self;
    pub fn requires<F: Fact>(&mut self) -> &mut Self;          // present at all
    pub fn requires_that<F: Fact>(&mut self, what: &'static str,
                                  holds: fn(&F) -> bool) -> &mut Self;
}
```

`declare` is required, so an implementation cannot silently forget it —
composite plugins like `HierarchicalRanker` in particular have to forward to
their parts. Having it on `Plugin` rather than on each axis trait lets the
engine collect every declaration through `&dyn Plugin`, whatever the axis.

A `RankingCriterion` is not a `Plugin`. All it can do is state, through
`RankingOrder`, which metric it sorts by, and the engine controls
`RankingOrder`; so `LexicographicalCriteria`, the `SliceRanker` that holds the
criteria, declares their metrics by running each criterion against a
`RankingOrder` that records instead of sorting.

Facts are provided by the phase's plugins and by its context: the competition,
or the output the previous phase's format declares, e.g. `EntrantsPerRanking(1)`.
A fact a plugin of the phase provides replaces the same fact of the context. The
check collects all provided facts into a type-keyed map built only for the
check (never stored or serialized) and evaluates every requirement against it.
It is flat: any plugin may require a fact of any other plugin or of the context,
and mutual requirements are fine, because a requirement states what must be true
of the phase, not what must have run first.

The vocabulary is open. Core defines facts about concepts it owns
(`MatchAnnotation<A>`, `EntrantsPerRanking`, ...); the crate
that owns a concept defines facts about it (a match plugin crate defines
`MatchDetail<Goals>`, which a goal-difference calculator in another crate
requires). Incompatibilities are unmet conditions on facts — monster DYP
`requires_that::<EntrantsPerRanking>("rankings of single players", ..)` —
never pairs of concrete types. A fact carries a value, so a requirement can ask
*how* as well as *whether*, and a fact holding a `fn` or a `Box<dyn Trait>`
carries behavior where a plugin needs some from another.

A fact provided twice within a phase is an error.

**A metric is a fact**, keyed by the metric type itself, and a calculator is an
ordinary plugin declaring that it provides it (`provides(Points::default())`).
There is no separate handling: a metric calculated by two calculators is a fact
provided twice (the disjointness of metrics), and a required metric no
calculator calculates is an unmet fact. That the declared metric is the one
`default_metric_value()` returns is a debug assertion when the phase starts.

**`compatibility` knows no axes.** It holds facts, declarations, the check and a
generic order of any list of plugins. The one place that knows the axes is
`PhasePlugins`, the plugins of a phase: it collects their declarations through
`&dyn Plugin`, resolves defaults and orders its calculators.

**Self-reported, except for criteria.** A criterion's requirement is recorded
from its call to `RankingOrder::by_metric::<M>()`, so it cannot drift from what
the criterion does. Everything else is self-reported in `declare`. That is the
trade-off accepted here: facts like "rankings are pairs" are not something the
compiler can derive, and none of the alternatives can express conditions on
values. Test helpers that build a phase from a plugin plus only the facts it
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

**Missing providers come from the catalog.** For a required fact nothing
provides, resolution asks the catalog for the registered plugins that provide it
with their default configuration, and adds the one it finds if there is exactly
one — `Matches` has exactly one calculator, so nobody is ever asked about it.
Several candidates are an ambiguity the engine must not settle, so it adds
nothing and the fact stays unprovided, as it does for a fact only a plugin that
has to be configured provides. Which kinds of plugin may be added is
`PhasePlugins`'s decision, expressed as `Accepts<K>` impls, so the resolution
itself knows no axis. Resolution repeatedly collects the declarations and
settles the first missing fact, each fact once, so it terminates even where
nothing is added. A plugin configured explicitly always wins, since the fact it
provides is never missing, and an added plugin is checked like every other one,
so one that does not fit the phase — e.g. a calculator that cannot read a
third-party match format — is reported as unmet rather than used. Consumers only
configure calculators that need their configuration (points per win, ...) or
that they want calculated without ranking by them.

**Resolution happens before the transition, validation in it.** `AddPhase::new`
resolves: it adds the missing plugins and sorts, and returns an error for a fact
provided twice or a cycle. A fact that stays unprovided is not its concern,
since the context may provide it. The transition then holds the complete, sorted
list, so neither the log nor a snapshot depends on how calculators were picked —
which it must not, because the catalog depends on which crates are linked in —
and changing a registration or the way of picking later never changes a replay.
Deserializing an `AddPhase` adds nothing — an unsorted list fails to load, and
a missing calculator fails the replay. Facts are checked in
`AddPhase::check_applicable`, where the phase's context is known; facts only
known at start, such as the number of entrants, are checked the same way at
`StartCompetition`. All of this runs once
per phase; the result-reporting path loops over an already sorted list and pays
nothing.

*Planned, once a metric has a second calculator:* a preference among the
candidates for one metric, so that resolution can settle what is an ambiguity
today — e.g. a match format offering the points calculator that understands its
matches. This is additive, and since resolution is not part of the log it can
change freely.

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
  could suggest different defaults for the same fact with nothing to settle
  which wins. Rejected; a requirement names a fact and nothing else.
- **Resolving inside `AddPhase::apply`.** Replay would then depend on which
  plugins are registered, so linking a crate in would change every saved log.
  Rejected.
- **Metrics as a kind of requirement of their own** (`required_metrics` next to
  `required`). Duplicated every rule for facts. Superseded.
- **The engine adding a calculator's metric to its declaration** from
  `default_metric_value()`. Structural, but made calculators a special case of
  collecting declarations. Superseded by calculators declaring their metric,
  checked by a debug assertion.
- **`declare` on each axis trait.** Repeated the same method per axis and left
  the engine no uniform way to ask a plugin. Superseded by `Plugin::declare`.
- **A blanket `impl<T: RankingCriterion> Plugin for T`** deriving a criterion's
  declaration from `order_rankings`. Works, but criteria are deliberately
  limited to what `RankingOrder` offers, so the holder of the criteria can
  declare for them without making them plugins. Rejected.
- **A boxed plugin as the default provider**, put into its slot by the
  resolution. `Box<dyn Plugin>` cannot become a `Box<dyn MetricsCalculator>`
  without the trait-to-trait casts `intertrait` existed for, and an enum over
  the axes makes the resolution axis-aware. Superseded by a registry per axis,
  which hands out a `Box<K::Object>`.
- **Optional accessor on the base trait** (`fn as_xyz(&self) -> Option<&dyn
  XYZ>`). Only core can add such accessors. Rejected as the general mechanism;
  still fine as a local pattern.

## The plugin catalog

*Partly implemented — the registries, `PluginEntry` and resolution reading them
exist. The schemas that let a UI ask for a plugin's parameters, the builder and
the defaults are the remaining steps of `PROPOSAL-plugin-catalog.md`.*

Every consumer that configures a phase has to know every plugin: the CLI holds a
table of labels, descriptions and default parameters, and a plugin of another
crate appears in none of them. The engine needs the same knowledge to fill a gap
a requirement leaves, which it had as a default provider per fact type.

### Decision

**A plugin registers itself, described.** One `PluginEntry<K>` per plugin holds
its id, its label, its description and, where the plugin needs no configuration,
a `fn() -> Box<K::Object>` creating it. Entries are collected with `inventory`,
so the catalog is whatever is linked in, plugins of other crates included, and
nothing enumerates them.

**One registry per axis, typed.** A `PluginKind` ties the axis trait to its
registry through `K::Object`, so the catalog hands out a `Box<dyn
MetricsCalculator>`, never something to downcast. `inventory` collects by type
and only into a type of the crate declaring the collection, so the registry type
is generated, by `plugin_kind!` next to the trait of the axis; that is the only
reason the macro exists. It writes the axis trait out, which is what lets a
registration name neither the entry type nor the kind twice:

```rust
plugin_kind! {
    pub MetricsCalculatorKind for dyn MetricsCalculator
}

inventory::submit! {
    MetricsCalculatorKind::register("…::points", "points", "scored for every win, …")
        .with_default(|| Box::new(PointsCalculator::new(1.0, 0.5)))
}
```

**What may be added is the composite's decision**, stated as `Accepts<K>` impls:
`PhasePlugins` accepts metrics calculators and nothing else, so "resolution adds
a second phase format" cannot be written, and resolution stays generic over the
axis.

**The ids are a contract** as soon as something outside the process names a
plugin — a saved configuration will, see the proposal. They are namespaced with
the crate the plugin lives in, and a plugin's id and its `typetag` tag are meant
to be the same string once the plugins get explicit tags.

**UI text lives on the plugin**, which means the engine ships English;
localisation would wrap it, keyed by the plugin id. It is what the CLI does
today, one crate further out.

### Alternatives

- **One global registry** with a `build` returning `Box<dyn Any>`. One
  `collect!` for everything instead of a generated type per axis, but trades a
  compile-time guarantee for a downcast. Rejected.
- **Generating the CLI's enums** from the plugin list. A consumer-side table
  either way, and no third-party plugin ever appears in it. Rejected.
- **A default provider per fact type**, see
  [Compatibility between plugins](#compatibility-between-plugins). Superseded.
- **A derive macro for the registration.** Worth it once the entry shape has
  stopped moving, not before. Deferred.

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

**`Ranking` stores its metrics in a `TypeMap`** — a `Map<TypeId, Box<dyn
TypeMapValue>>` that serializes as a plain sequence, since each value carries its
own `typetag` tag. `Ranking::metric::<T>()` resolves to `Option<&T>` via
`TypeMap::get`, which uses `downcast_unchecked_ref` because the key already
proves the concrete type. Keyed by type rather than a `Vec` scan, so a ranker
that reads several metrics per comparison pays a hash lookup, not a linear
search.

**`MetricsCalculator` computes incrementally**, rather than recomputing a whole
ranking from scratch:

```rust
#[typetag::serde(tag = "type", content = "value")]
pub trait MetricsCalculator: Plugin {
    fn default_metric_value(&self) -> Metric;
    fn update_after_match_result_changed(
        &mut self, phase_data: &PhaseData, r#match: &Match,
        old_match_result: MatchResult,
        old_metrics: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)>;
    fn update_after_bye(
        &mut self, phase_data: &PhaseData, bye: &Bye,
        old_metrics: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)>;
}
```

(`update_after_bye` is explained in [Byes](#byes).)

`default_metric_value()` both seeds every ranking at phase start and declares
which metric type this calculator owns, so disjointness is checkable without
running any computation. `old_metrics` lets a calculator that depends on another
calculator's output (e.g. Buchholz on points) read the pre-update values of the
calculators that ran before it, while `PhaseData` holds their new ones. Both are
only complete if those calculators did run before it, so calculator order does
matter; it is derived from their declared dependencies, see
[Compatibility between plugins](#compatibility-between-plugins).

**Ranking comparison stays typed.** A `Ranker` orders rankings; the implemented
one, `HierarchicalRanker`, applies a list of `SliceRanker`s, each resolving ties
left by the previous. `LexicographicalMetrics` extracts one dense, typed column
per metric up front via `MetricGetter`/`MetricColumn`, so a comparison indexes
into values already known to be of that metric's type instead of downcasting per
comparison — this runs on every reported result, so it is worth resolving once.

*Planned:* the active calculators' metric types being pairwise disjoint, and
metrics required by other plugins being present, are checked as facts once per
phase, see [Compatibility between plugins](#compatibility-between-plugins).
`AddPhase` currently only checks that the competition has not started.

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

## Consumer data on entities

Consumers want to attach data core never reads — a display name, a table
assignment, a database id.

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
  `entrant.annotation::<DisplayName>()`.
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

This requires `TypeMapValue: DynClone`, because `Transition::apply` takes
`&self` and so must clone its payload on every replay. That ripples into
`Metric`, but every metric type is a trivially-`Clone` plain struct, and it
mirrors `Plugin: Debug + DynClone`. It also costs `Entrant` its `PartialEq`
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
  is not in the snapshot and not in the log, so names must be persisted
  separately, kept from drifting, and are untouched by undo. Superseded once both
  persistence paths became requirements.
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
scores them with `points_for_bye`, 0 unless set. Adding up points result by
result would drift for scores a float cannot represent exactly.

*Not covered yet:* kinds of byes worth different points (a requested half-point
bye, a sit-out scored with average points). A requested bye is transition data
the format has to accept, so it is a feature of its own, and the kind would then
need a vocabulary calculators can read. Forfeits are not byes: they have an
opponent and are a match result.

### Alternatives

- **One enum `Match | Bye` in and out**, i.e. formats emit it and calculators
  receive it in place of the match. Calculators would need a `Bye` arm in
  `update_after_match_result_changed` that can never run, since nobody reports a
  result for a bye, and one collection of both would mix byes into the match
  numbers. Rejected; the format's side of it survives as `Schedule`.
- **A bye as a degenerate match** (no opponent, or a preset result). Every
  calculator would see byes through the existing path and silently get them
  wrong: Buchholz looking up an opponent, `Matches` counting it as played. Every
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
tag:

```rust
#[typetag::serde(tag = "type", content = "value")]
pub trait MetricsCalculator: Plugin { /* ... */ }

#[typetag::serde(name = "tournament_engine::buchholz")]
impl MetricsCalculator for BuchholzCalculator { /* ... */ }
```

*Outstanding:* no plugin or metric impl names its tag yet — all use bare
`#[typetag::serde]`, so every tag on the snapshot path is currently the default
bare type name (`"Buchholz"`, `"BuchholzCalculator"`, ...). That is both the
collision risk this decision meant to avoid and a silent break of old snapshots
on any type rename. Worth fixing before the first snapshot anyone needs to keep.
The log path does not share the problem: every `CompetitionTransition` impl names
its tag.

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
holds how that state came about, so undo — and undoing an undo — still works
after a restore.

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

## Dependencies this implies

- `typetag` (plus its internal `inventory` and `erased_serde`) and `serde` for
  serializing trait objects; `postcard` and `serde_json` exercise both a
  non-self-describing and a self-describing format in tests.
- `dyn-clone` for cloning boxed plugins, and (*planned*) boxed `TypeMapValue`s.
- `indexmap` for deterministic iteration order, with `rustc-hash` as a faster
  hasher for small `Copy` id keys.
- `uuid` (v5) for content-derived ids; `rand` and `rand_seeder` for seeded,
  reproducible shuffling; `vec1` for non-empty collections.
- No `enum_dispatch`, `linkme`, `ambassador` or `intertrait`: each was only
  needed for an alternative ruled out above.
