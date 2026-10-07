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
(with `SliceRanker`, `MetricGetter` beneath it), `MetricsCalculator`. All share
the `Plugin` supertrait (`Debug + DynClone`), and each has a hand-written
`impl Clone for Box<dyn Trait>` delegating to `dyn_clone`.

This applies uniformly to future axes: each one gets a trait and a slot, with no
per-axis judgment call. A string-keyed registry for loading a configuration from
non-Rust sources can be layered on additively later; it is not a v1 requirement.

### Alternatives

- **Closed `Kind` enum per axis.** Allows static dispatch via `enum_dispatch`,
  but third parties cannot add variants. Rejected.
- **Generic type parameters per axis.** Zero-cost, but conflicts with runtime
  selection, and every combination becomes a distinct type. Rejected.
- **Name-driven registry from the start** (`inventory`/`linkme`). Needed only
  for config-file-driven selection, which v1 doesn't have. Deferred, not
  rejected.

## Compatibility between plugins

*Planned — `intertrait` is not a dependency yet and `Plugin` carries no
`requirements()`.*

Not every combination of plugins is valid, and detecting that must not rely on a
hardcoded list of concrete type pairs, which would be blind to third-party
implementations.

### Decision

Marker traits registered with `intertrait`, one mechanism for both plain yes/no
compatibility and compatibility that needs extra behavior — "does this implement
`XYZ`" is already a yes/no answer, and giving `XYZ` methods adds richer behavior
for free.

**Providing** a capability is structural, not self-reported: a type provides
`XYZ` by implementing it and registering the cast with `#[cast_to(XYZ)]`. There
is no `capabilities()` list that can drift from what the type actually does.

**Requiring** a capability is a type-erased predicate, so the central check never
names a concrete axis or marker trait:

```rust
pub struct Requirement {
    label: &'static str,                // for error messages
    satisfied_by: fn(&dyn Any) -> bool,
}

impl Requirement {
    pub fn implements<T: ?Sized + 'static>(label: &'static str) -> Self {
        Self { label, satisfied_by: |any| any.cast::<T>().is_some() }
    }
}
```

Every axis trait gains `as_any()` and `requirements() -> Vec<Requirement>`, so
one check can walk all configured plugins homogeneously and verify every
requirement is satisfied by some other plugin.

Known caveat before implementing: `intertrait`'s registry is keyed by concrete
`TypeId`, so `#[cast_to]` cannot register a generic impl for all its
instantiations. Marker traits must be implemented on concrete types.

### Alternatives

- **Closed `Capability` enum owned by core.** Cheap `HashSet` checks, but the
  vocabulary is closed to core, and `capabilities()` is a hand-maintained list
  that can drift from the actual impls. Rejected.
- **Hardcoded match over concrete type pairs.** Invisible to third-party types.
  Rejected.
- **`Any` + downcast to a concrete type.** Cannot generically ask "does this
  implement `XYZ`" without already knowing every concrete type that might.
  Rejected as insufficient alone.
- **Optional accessor on the base trait** (`fn as_xyz(&self) -> Option<&dyn
  XYZ>`). The base trait must be edited per marker trait, so only core can add
  optional interfaces. Rejected as the general mechanism; still fine as a local
  pattern.

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
        &self, phase: &Phase, r#match: &Match,
        old_match_result: MatchResult,
        old_metrics: &Map<TypeId, Vec<(RankingId, Metric)>>,
    ) -> Vec<(RankingId, Metric)>;
}
```

`default_metric_value()` both seeds every ranking at phase start and declares
which metric type this calculator owns, so disjointness is checkable without
running any computation. `old_metrics` lets a calculator that depends on another
calculator's output (e.g. Buchholz on points) read the pre-update values,
making the order calculators run in irrelevant to the result.

**Ranking comparison stays typed.** A `Ranker` orders rankings; the implemented
one, `HierarchicalRanker`, applies a list of `SliceRanker`s, each resolving ties
left by the previous. `LexicographicalMetrics` extracts one dense, typed column
per metric up front via `MetricGetter`/`MetricColumn`, so a comparison indexes
into values already known to be of that metric's type instead of downcasting per
comparison — this runs on every reported result, so it is worth resolving once.

*Planned:* validate in `AddPhase::check_applicable` that the active calculators'
metric types are pairwise disjoint, and that metrics required by other plugins
are present. Both are declared-set checks run once per phase, not per
recalculation. `AddPhase` currently only checks that the competition has not
started.

### Alternatives

- **String-keyed metrics.** Untyped bag. Rejected.
- **A dedicated `RankData` trait with `cmp_for_leaderboard`** (an earlier draft
  of this design). Gave core one fixed comparison per ranking type; the
  `SliceRanker` chain gives composable tiebreaking instead, and `TypeMap` already
  supplies typed access. Superseded.
- **Metrics as marker traits checked via `Requirement`.** That mechanism answers
  "does this instance implement behavior X", not "what data did calculating for
  this ranking produce", and offers no way to express disjointness. Rejected as a
  different concern.
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
   `&Phase`, so `Phase<M, R>` makes `PhaseFormat<M, R>` generic.
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
arrives as transition data, and why a match number is positional rather than
durable.

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
- *Planned:* `intertrait` for the compatibility mechanism.
- No `enum_dispatch`, `linkme` or `ambassador`: each was only needed for an
  alternative ruled out above.
