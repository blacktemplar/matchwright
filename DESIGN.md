# Tournament Engine — Design Plan

A generic core library for running 1-on-1 match tournaments (single elimination,
round-robin, Swiss, ...), where key components are pluggable and selected at
runtime, with compatibility rules between them.

## Pluggable axes

The library is built around independent "axes" of behavior that vary
separately — for example, the ranking system (how ranks get calculated from
match results), the match outcome (how a match result is represented and
resolved), and the competition mode (single elimination, round-robin,
Swiss, ...). This list is not necessarily exhaustive; more axes may turn up
during implementation. Each axis needs to be selectable at runtime, and
different consumers of the library need to be able to plug in their own
implementations for a given axis.

### Requirements

- Each axis's implementations must be independently pluggable at the code
  level: writing or choosing an implementation for one axis must never
  require modifying another axis's trait or implementations. This is
  about implementation independence, not behavioral independence — it
  does not mean every combination of choices across axes is valid. The
  system still needs a way to express and validate compatibility
  constraints between specific choices on different axes (see
  "Compatibility rules between components" below); that validation must
  itself stay outside the axis traits themselves, so it doesn't undermine
  this independence at the code level.
- The concrete implementation for each axis is chosen at runtime (not
  baked in via generics/monomorphization at compile time).
- Third parties (crates outside this one) must be able to add their own
  implementations for an axis and use them together with this library,
  without needing to modify this library's source.
- New axes may be discovered later during implementation; the mechanism
  chosen must apply uniformly to any future axis, not require a fresh
  design decision each time one is added.
- For v1, selection only needs to happen via direct Rust construction
  (a consumer builds a configuration in code). Loading a configuration
  from an external, non-Rust source (e.g. a config file naming an
  implementation by string) is explicitly not a v1 requirement.

### Proposed Solution

Every axis is modeled as its own trait (the Strategy pattern applied per
axis), and every axis is **open**: implementations are selected as
`Box<dyn Trait>`, and the trait itself is public and unsealed so anything
outside this crate can implement it. A consumer wires a tournament together
by constructing the chosen implementations directly and handing them to a
builder:

```rust
let config = TournamentConfig::builder()
    .ranking(Box::new(EloRanking::default()))
    .match_outcome(Box::new(BestOfThree::default()))
    .competition_mode(Box::new(SingleElimination::default()))
    .build()?;
```

No name-based registry and no `inventory`/`linkme` dependency is needed for
this — selection is just ordinary Rust construction. This satisfies the
"applies uniformly to future axes" requirement because there is no
per-axis judgment call to make (unlike a mixed open/closed design): every
new axis just gets a new public trait and a new builder slot.

A string/config-driven registry can be layered on top later, additively,
without changing this design — it would just be a lookup table mapping
names to constructors that produce the same `Box<dyn Trait>` values
consumed here.

### Alternatives

- **Closed `Kind` enum + match, per axis** (e.g. `enum RankingKind { Elo,
  Glicko2 }` matched to constructors). *Advantage:* compile-time
  exhaustiveness checking; can use `enum_dispatch` for static-dispatch
  performance instead of `dyn` vtables. *Disadvantage:* closed by
  construction — third parties cannot add variants, which directly
  violates the extensibility requirement. Rejected for that reason.
- **Generic type parameters** (`Tournament<R: RankingSystem, M: MatchOutcome,
  C: CompetitionMode>`), resolved at compile time. *Advantage:* zero-cost,
  fully static dispatch. *Disadvantage:* the concrete types must be known
  at compile time, which conflicts with the "chosen at runtime" requirement,
  and every combination of axis choices becomes a distinct monomorphized
  type, which is awkward to store, pass around, or choose between
  dynamically. Rejected.
- **Name/config-driven plugin registry from the start** (`inventory`/
  `linkme`, string-keyed lookup). *Advantage:* supports loading a
  tournament configuration from external config/data. *Disadvantage:*
  real added complexity and a new dependency for a requirement v1
  explicitly doesn't have. Deferred, not rejected — can be added later as
  an additive layer.

## Compatibility rules between components

Not every combination of axis implementations is valid — e.g. a ranking
system that requires deterministic pairing may be incompatible with a
competition mode that doesn't provide it. The library needs a way to
detect invalid combinations, ideally at configuration-build time rather
than deep inside tournament logic.

### Requirements

- Detecting an incompatible combination must not require a hardcoded list
  of concrete type pairs in this crate, because that list would be blind
  to any third-party implementation (violates the axis openness
  requirement above).
- Must support simple yes/no compatibility checks (e.g. "these two are
  just incompatible").
- Must also support compatibility that isn't just a bool: cases where,
  once two components are confirmed compatible, one needs to actually
  invoke extra behavior on the other through a richer interface.
- Whatever mechanism is used must work identically for core-provided and
  third-party implementations, with no special-casing.

### Proposed Solution

A single mechanism — marker traits registered with `intertrait` — covers
both plain yes/no compatibility and compatibility that needs extra
behavior, because "does this implement marker trait `XYZ`" is already a
yes/no answer on its own; giving `XYZ` real methods just adds richer
behavior on top for free. (An earlier version of this section proposed a
separate closed, core-owned `Capability` enum for the yes/no case —
rejected, see Alternatives — in favor of this single mechanism.)

**Providing a capability is a structural fact, not a self-reported one.**
A component "provides" capability `XYZ` simply by implementing the marker
trait and registering the cast:

```rust
trait XYZ { fn extra_method(&self) -> /* ... */; } // can be empty, for pure yes/no

#[cast_to(XYZ)]
impl XYZ for BestOfThree { /* ... */ }
```

There's no separate `capabilities()` method to keep in sync with what a
type actually implements. A hand-maintained list is exactly the kind of
manually-reported fact that can silently drift from reality; making
"provides" a structural property means the compiler and `intertrait`'s
registry agree on it by construction, not by convention.

**Requiring a capability is a type-erased predicate, so the central check
stays generic.** A component that needs some *other* component to support
`XYZ` declares it as a small `Requirement` value that captures the target
marker trait once, at the point where it's already named:

```rust
pub struct Requirement {
    label: &'static str,               // for error messages
    satisfied_by: fn(&dyn Any) -> bool,
}

impl Requirement {
    pub fn implements<T: ?Sized + 'static>(label: &'static str) -> Self {
        Self { label, satisfied_by: |any| any.cast::<T>().is_some() }
    }
}

trait MatchOutcome: Any {
    fn as_any(&self) -> &dyn Any;
    fn requirements(&self) -> Vec<Requirement> { vec![] }
    /* ... */
}
```

Every axis trait gets the same minimal `as_any()` (the same pattern
already used for `RankData` below), so `TournamentConfig::build()` can
walk *all* configured axis instances homogeneously — regardless of which
axis trait each one implements — and check each `Requirement` against
every other instance:

```rust
fn build(&self) -> Result<Tournament, ConfigError> {
    let components: Vec<&dyn Any> = /* as_any() of every configured axis instance */;
    for component in &components {
        for requirement in /* that component's requirements() */ {
            if !components.iter().any(|c| (requirement.satisfied_by)(*c)) {
                return Err(ConfigError::MissingCapability(requirement.label));
            }
        }
    }
    Ok(/* ... */)
}
```

This is the same "requirements ⊆ provided" check as before — now checked
per-instance via `intertrait` instead of `HashSet<Capability>` equality —
and it stays uniform for core and third-party implementations, since
neither `Requirement::implements` nor the `build()` loop ever names a
concrete axis or marker trait.

### Alternatives

- **Closed, core-owned `Capability` enum** (what this section previously
  proposed):
  ```rust
  #[non_exhaustive]
  enum Capability { SupportsDraws, RequiresDeterministicPairing, StatefulMatches, /* ... */ }
  fn capabilities(&self) -> HashSet<Capability>; // what it provides
  fn requirements(&self) -> HashSet<Capability>;  // what it needs from other axes
  ```
  *Advantage:* cheap `HashSet`-based checks; `#[non_exhaustive]` lets core
  add variants in a minor version without a breaking change; every known
  capability is enumerable in one place. *Disadvantage:* the vocabulary is
  closed to core — two third-party crates wanting to express a
  compatibility concept core never anticipated have no way to add a
  variant without a core release, which directly contradicts the "third
  parties add implementations without modifying this library's source"
  requirement. It also lets `capabilities()` drift from what a type
  actually does, since it's a hand-maintained list rather than a
  structural fact — a general pattern this design otherwise avoids (see
  the `undoable-state-machine` `Transition` design, which rejected a
  self-reported boolean for the same reason). Rejected in favor of the
  marker-trait mechanism above, which is open by construction and ties
  "provides" directly to the actual `impl`.
- **Hardcoded match over concrete types** (`match (ranking.type_id(),
  mode.type_id()) { ... }`). *Advantage:* simplest to write for a small,
  fixed set of implementations. *Disadvantage:* directly violates the
  "no hardcoded pairs" requirement — invisible to third-party types.
  Rejected.
- **`HashMap<String, Value>` extra-data bag** attached to each component,
  inspected for compatibility. *Advantage:* maximally flexible, no schema
  needed up front. *Disadvantage:* loses all type information; turns
  compile-time mistakes (wrong key, wrong shape) into runtime lookup
  failures; doesn't actually solve the "invoke extra behavior" requirement
  since values, not methods, live in the map. Considered an anti-pattern
  for this use case and rejected.
- **`std::any::Any` + downcast to a concrete type.** *Advantage:*
  standard library only, no extra dependency. *Disadvantage:* lets you
  downcast to a concrete type you already know about, but not generically
  ask "does this implement trait `XYZ`" without already knowing every
  concrete type that might implement it — doesn't solve the third-party
  case. Rejected as insufficient on its own.
- **Manual optional accessor method on the base trait**
  (`fn as_xyz(&self) -> Option<&dyn XYZ> { None }`, overridden where
  supported). *Advantage:* zero extra dependencies, simple to understand.
  *Disadvantage:* the base trait must be edited to add an accessor for
  every new marker trait, so only core can introduce new optional
  interfaces — a third party inventing a new marker trait has no way to
  expose it. Rejected as the general mechanism, but still the right choice
  as a narrow, local pattern if a specific marker trait is expected to be
  wrapped generically (see previous discussion); not needed for v1 since
  wrapping is out of scope (below).

## Entity data owned by axis implementations

Some axis implementations don't just provide behavior — they also define
the shape of data attached to related entities. A `RankingSystem`
implementation determines what a `Rank` contains: an Elo-based system
might store a single rating float, while a system with tiebreakers might
store wins, losses, and game difference. That shape can even depend on
what *another* axis's implementation exposes — a ranking system that
tracks game difference needs the match outcome to expose per-game
win/loss counts, which not every `MatchOutcome` implementation provides.

### Requirements

- Each `RankingSystem` implementation (and, in general, any axis whose
  implementations produce entity data) must be able to define its own
  data shape for the entity it produces, without core knowing all
  possible shapes in advance — the same third-party-openness requirement
  the axis traits themselves have.
- Since axis implementations are stored as `Box<dyn Trait>` for runtime
  pluggability, the produced entity type can't be exposed as a plain
  per-implementation associated type: a caller holding only
  `Box<dyn RankingSystem>` needs a concrete return type to work with, not
  a family of unrelated per-implementation types known only at compile
  time.
- Core still needs a minimal, uniform way to work with *any* produced
  entity regardless of which implementation produced it (e.g. sorting
  `Rank`s for a leaderboard).
- A `RankingSystem` implementation must be able to pull richer,
  implementation-specific data out of another axis's implementation (e.g.
  per-game scores from a match outcome) when it's available, and this
  dependency should be checkable rather than silently producing
  incomplete data.

### Proposed Solution

Represent axis-produced entity data itself as a trait object, reusing the
same open-trait-plus-optional-richer-interface pattern already used for
the axes themselves:

```rust
trait RankData {
    fn as_any(&self) -> &dyn Any;
    fn cmp_for_leaderboard(&self, other: &dyn RankData) -> Ordering;
}

trait RankingSystem {
    fn update(&self, current: &dyn RankData, outcome: &dyn MatchOutcome) -> Box<dyn RankData>;
    ...
}
```

`RankData` is small and object-safe, giving core just enough to do generic
things (sort a leaderboard) without knowing the concrete type. A specific
`RankingSystem` implementation defines its own concrete `RankData` type
with whatever fields it needs. Downstream code that wants the concrete
shape can downcast via `as_any` if it already knows the concrete type
(e.g. the same crate that provides the `RankingSystem`); for an optional
richer view meant to be shared *across* implementations, define a marker
trait and use `intertrait` casting exactly as in the compatibility
section — e.g. `dyn HasGameDifference { fn game_difference(&self) -> i32;
}` — so a display layer can ask "does this `Rank` expose game difference"
without knowing every concrete `RankData` type that might answer yes.

The cross-axis dependency uses the identical mechanism: a `RankingSystem`
implementation that wants per-game data casts the `&dyn MatchOutcome` it's
given to a marker trait such as `dyn MultiGameOutcome { fn games_won(&self)
-> u32; fn games_lost(&self) -> u32; }`. If the cast succeeds, it populates
those fields in the `Rank` it produces; if not, it can't. This is the same
marker-trait-casting pattern already designed for cross-axis
compatibility, just applied to data extraction instead of a validity
check — no new machinery. To avoid silently producing a degraded `Rank`
when the match outcome doesn't support the needed data, this
`RankingSystem` implementation should also declare
`Requirement::implements::<dyn MultiGameOutcome>("MultiGameOutcome")` in
its `requirements()`, so an incompatible pairing is caught by
`TournamentConfig::build()` rather than discovered as missing data at
runtime.

### Alternatives

- **Associated type on the axis trait**
  (`trait RankingSystem { type Rank; fn update(...) -> Self::Rank; }`).
  *Advantage:* fully typed, no downcasting anywhere — the most idiomatic
  Rust way to say "this implementation determines a related type."
  *Disadvantage:* breaks object safety in the way needed here — a
  `Box<dyn RankingSystem>` can't carry a per-instance associated type, so
  this is incompatible with the "chosen at runtime, stored as `dyn
  Trait`" decision already made for all axes. Rejected for that reason,
  not because the pattern is bad in general.
- **Fixed core-defined `Rank` struct** covering common fields (rating,
  wins, losses, game difference, ...) used by every ranking system.
  *Advantage:* simplest possible core — no trait objects or downcasting
  needed for `Rank` itself. *Disadvantage:* doesn't scale: every new
  ranking system either overloads existing fields for unrelated meanings
  or needs core to grow the struct, and a third-party ranking system with
  a genuinely novel data need has nowhere to put it. Same shape of
  problem as the rejected metadata bag, just relocated into core's struct
  definition. Rejected.
- **`Box<dyn Any>` with no dedicated `RankData` trait**, downcast to a
  concrete type wherever needed. *Advantage:* even less machinery than
  the proposed `RankData` trait. *Disadvantage:* gives core, and any
  third-party consumer that doesn't know the concrete type, nothing to do
  with a `Rank` at all — not even sortable for a leaderboard — since
  `Any` alone provides no common contract. Rejected in favor of a minimal
  `RankData` trait that at least supplies the operations core actually
  needs.

## Extra data on components

Consumers commonly want to attach information to a component that core's
algorithms never need — a display name, a database id, UI state, and so
on.

### Requirements

- Must not require modifying or even being aware of, from core's
  perspective, the extra data a consumer wants to attach.
- Must not require core algorithms to carry or thread through data they
  don't use.
- Should not reintroduce the type-safety problems already ruled out above
  (no untyped data bags).

### Proposed Solution

Core assigns stable identifiers to entities (players, matches, ...) as
plain newtypes, e.g. `pub struct PlayerId(u64)` (`Copy + Eq + Hash`),
generated when an entity is registered. The consuming crate keeps its own
side-table keyed by that id — `HashMap<PlayerId, PlayerMetadata>` — for
anything not consulted by core algorithms. Core never needs to know this
data exists, so no core trait, struct, or algorithm changes when a
consumer adds new metadata fields.

### Alternatives

- **Wrapping a core implementation to add data/behavior** (a decorator
  struct holding the core implementation plus extra fields, forwarding
  trait methods by hand or via the `ambassador` crate). *Advantage:* keeps
  the extra data attached directly to the object, useful if the data must
  travel with the value through arbitrary APIs. *Disadvantage:* Rust has
  no built-in trait delegation, so every trait method must be forwarded
  (mitigated but not eliminated by `ambassador`); and it interacts badly
  with the `intertrait` mechanism above, since `intertrait`'s cast
  registry is keyed by concrete `TypeId` — a generic wrapper `Wrapper<T>`
  does not automatically preserve `T`'s registered casts. Explicitly ruled
  out for v1: not just deprioritized, but a deliberate non-goal until a
  concrete need appears that side-tables can't cover.
- **`HashMap<String, Value>` field bag on the component itself.** Same
  rejection as in the compatibility section — loses type safety for no
  offsetting benefit here, since the data is inert (never consulted by
  core).

## Serializing pluggable state

`Competition` needs to be persistable as a snapshot — not just
reconstructable by replaying its transition log from scratch — and that
snapshot must remain readable by a later build of the program (a
recompile, a dependency bump, a redeploy), as long as the concrete plugin
types involved are still linked in.

### Requirements

- `Competition`, its `Phase`s, `Ranking`s, and every axis instance
  embedded in them (`PhaseFormat`, `MetricCalculator`, the metrics
  attached to a `Ranking`, and any future axis stored the same way) must
  be serializable and deserializable as a snapshot.
- The serialized form must not depend on anything that's only stable
  within a single process — this rules out raw `std::any::TypeId`, which
  is not guaranteed stable across compiler versions, dependency versions,
  or rebuilds of identical source (see prior discussion).
- Must stay open: a third-party crate providing an axis implementation
  (or a `Metric`) must be able to make its own types serializable without
  this crate knowing about the concrete type in advance — the same
  third-party-openness requirement as every other mechanism in this
  document.
- Serialization should be reasonably compact: avoid the overhead of a
  verbose self-describing text format (JSON) and avoid the identifier
  defaulting to a full Rust type path when a short, deliberately-chosen
  one is just as usable.
- A tag collision between two independently-developed crates (both
  registering the same identifier for different types) must not silently
  deserialize into the wrong concrete type.

### Proposed Solution

Use the [`typetag`](https://docs.rs/typetag) crate. Each pluggable
implementation is annotated on the `impl` block with a short, explicitly
chosen tag (not the default, which is the bare type name — a namespaced
name lowers collision odds with an unrelated crate's type of the same
short name):

```rust
#[typetag::serde(name = "tournament_engine::game_difference")]
impl Metric for GameDifferenceMetric { /* ... */ }
```

and the trait itself opts in once:

```rust
#[typetag::serde(tag = "type", content = "value")]
pub trait Metric: Debug {
    fn as_any(&self) -> &dyn Any;
}
```

This gives `Box<dyn Metric>` (and any other axis trait annotated the same
way — `PhaseFormat`, `MetricCalculator`, ...) a working `Serialize`/
`Deserialize` impl, backed by an `inventory`-populated registry mapping
tag → concrete type, built and consulted entirely inside `typetag` —
no bespoke registry code needed in this crate.

Verified directly against `typetag`'s source (not just its docs) for two
things this document previously got wrong or left unconfirmed:

- **It works with non-self-describing binary formats, including
  Bincode** — but only when the trait is *adjacently* tagged, which is
  why the annotation above carries a `content` as well. Unlike serde's
  built-in internally/adjacently-tagged enum representations (which do
  require a self-describing format, because they buffer an unknown shape
  via `deserialize_any` before knowing the target type), `typetag` reads
  the tag and content as two ordinary, positionally-decoded fields, then
  uses a `DeserializeSeed` — chosen *after* the tag is already known — to
  decode the second field as the right concrete type. No generic
  buffering, no `deserialize_any`, so non-self-describing formats work
  fine.

  Internal tagging (`tag` without `content`) does *not* work, because
  `typetag` serializes the content through an adapter that reports
  `Serializer::is_human_readable() == true` whatever the real format
  says, while deserialization sees the real format. A value below that
  content whose representation depends on the flag — the `Uuid` of any id
  in this crate, for instance — is then written as a string and read back
  as raw bytes, which fails. Adjacent tagging keeps the content out of
  that adapter; the cost in JSON is one nesting level
  (`{"type": "...", "value": {...}}` instead of a flat object), and it
  halves the postcard payload compared to working around the defect by
  giving ids a format-independent string representation. Upstream bug,
  reported against `typetag` 0.2.23.
- **A tag collision fails loudly, not silently.** The generated registry
  is a `BTreeMap<&'static str, Option<DeserializeFn<T>>>`. The first
  registration of a tag stores `Some(deserialize_fn)`; a second
  registration of the *same* tag overwrites that entry with `None`.
  Deserializing a value whose tag maps to `None` returns an explicit
  error ("non-unique tag of ...") instead of picking either
  implementation. Preventing the collision in the first place is still on
  the implementor (pick a distinctive, namespaced tag), but a collision
  that does happen can't cause silent misdirection to the wrong type.

The one thing `typetag` doesn't give you is a truly fixed-size binary
discriminant — the tag is always a string, so wire cost scales with the
tag's length. The mitigation is the same explicit, short, namespaced tag
already shown above; a few extra bytes per serialized value is an
acceptable cost for this crate's actual workload (occasional persistence
of tournament state, not a high-frequency hot path) against the
alternative of maintaining custom serialization plumbing ourselves.

### Alternatives

- **`std::any::TypeId`** (the identifier used before this topic was
  settled). *Advantage:* free, compiler-guaranteed, zero chance of
  accidental collision between unrelated types, no naming step for
  implementors. *Disadvantage:* only stable within a single process/build
  — cannot survive a `Competition` snapshot being read back after a
  recompile or redeploy. Rejected now that whole-`Competition`
  serialization is a settled requirement.
- **A hand-rolled `TypeTag([u8; 8])` + `erased_serde` + `inventory`
  registry** (an earlier draft of this section). *Advantage:* a genuinely
  fixed-size binary discriminant instead of a string, shaving a few bytes
  per serialized value. *Disadvantage:* reimplements, with less testing
  and no upstream maintenance, substantially the same mechanism `typetag`
  already provides (including the collision-detection behavior above) —
  once `typetag` was confirmed to already support non-self-describing
  formats, the only remaining advantage was the smaller tag, which wasn't
  worth owning bespoke registry/framing code for. Rejected in favor of
  reusing `typetag`, consistent with this crate's existing preference for
  a mature dependency (`intertrait`) over hand-rolling the equivalent
  open-registry mechanism for cross-axis compatibility.
- **Auto-derived tag** (e.g. a compile-time hash of the fully-qualified
  type name, computed by a proc macro, instead of an explicit
  `#[typetag::serde(name = "...")]`). *Advantage:* implementors can't
  forget to assign a tag. *Disadvantage:* renaming the type or moving it
  to a different module silently changes the derived tag, breaking old
  serialized data with no warning — the same "silently drifting fact"
  failure shape already rejected for the `Capability` enum's
  self-reported `capabilities()`, and worse here since there's no
  explicit place the stability contract is written down. Rejected in
  favor of an explicit tag that makes "never change this" a visible,
  deliberate commitment in the source.

## Ranking metrics

A `Phase`'s `Ranking`s need to carry data used to sort/rank entrants —
things like wins, losses, game difference, Buchholz score, etc. Which of
these are computed and attached varies per phase (e.g. Swiss wants
Buchholz, single elimination doesn't), and third parties should be able to
add entirely new metrics without modifying core.

### Requirements

- For each `Phase`, before the competition starts, a set of
  `MetricCalculator`s is chosen to be active for that phase. Any number of
  calculators may be active at once.
- A single `MetricCalculator` may calculate more than one metric.
- The metrics produced by the calculators active on a given phase must be
  disjoint — no two active calculators may both produce the same metric.
- Third parties must be able to implement new `MetricCalculator`s (open
  axis, same as `PhaseFormat` and the other axes above), without modifying
  this crate.
- When a `Ranking` is (re)computed, every active `MetricCalculator` is
  invoked and its output metrics are attached to the ranking.
- Other plugin types (e.g. a `PhaseFormat`) may depend on a specific
  metric being available among the phase's active calculators — this
  needs to be checkable, not just discoverable as missing data at
  runtime.
- A `MetricCalculator` may also be incompatible with other plugins for
  reasons unrelated to any specific metric (general plugin-to-plugin
  incompatibility), not just the metric-disjointness case above.

### Proposed Solution

**Metrics are identified by Rust type, not by name.** Each metric is its
own small type implementing an object-safe marker trait:

```rust
#[typetag::serde(tag = "type")]
trait Metric: Debug {
    fn as_any(&self) -> &dyn Any;
}
type MetricId = TypeId; // TypeId::of::<SomeMetric>()
```

`MetricId` stays `TypeId` here — deliberately distinct from the
`typetag`-based identity used to *serialize* a `Box<dyn Metric>` (see
"Serializing pluggable state" above). Disjointness- and
requirement-checking (below) never touches serialized data: it's a
structural comparison run fresh, in-process, every time a `Phase` is
configured or replayed, on live `Box<dyn MetricCalculator>` values —
exactly the case where `TypeId` is stable and free. Reusing `TypeId` here
keeps the set of metrics open — any third party can define a new
`Metric`-implementing struct without a closed enum — without
reintroducing the untyped `HashMap<String, Value>` bag already rejected
elsewhere in this document.

**`MetricCalculator` is another open axis, shaped like `PhaseFormat`:**

```rust
trait MetricCalculator: Debug + DynClone {
    /// Declared statically, so disjointness can be validated without
    /// running any computation.
    fn produced_metrics(&self) -> HashSet<MetricId>;
    fn calculate(&self, phase: &Phase) -> HashMap<PhaseTeamId, Vec<Box<dyn Metric>>>;

    // General plugin-to-plugin (in)compatibility unrelated to a specific
    // metric reuses the same marker-trait mechanism as every other axis.
    // "Provides" needs no method here — it's just implementing (and
    // #[cast_to]-registering) whatever marker trait applies.
    fn as_any(&self) -> &dyn Any;
    fn requirements(&self) -> Vec<Requirement> { vec![] }
}
```

`Phase` gets `metric_calculators: Vec<Box<dyn MetricCalculator>>`, set when
the phase is added.

**Disjointness and cross-plugin metric dependencies are validated once, in
the `AddPhase` transition's `check_applicable`** — the same point
`PhaseFormat` is attached to a `Phase` — rather than on every ranking
recalculation, so there's no repeated runtime cost once a phase is
configured:

- Disjointness: fold `produced_metrics()` across all active calculators
  into a `HashSet`, rejecting on the first collision. This stays a plain
  `HashSet<MetricId>` check, not a `Requirement` — it's about which
  *data-shaped types* get produced, not about any instance implementing
  a behavioral marker trait, so it doesn't fit the compatibility
  mechanism above.
- Cross-plugin metric requirements: any axis (e.g. `PhaseFormat`) gains
  `fn required_metrics(&self) -> HashSet<MetricId> { HashSet::new() }`,
  checked as a subset of the phase's union of `produced_metrics()` — same
  "declared set, checked once centrally" shape as disjointness above,
  just as a subset check instead of a pairwise-disjoint one.
- General plugin-to-plugin incompatibility not tied to a specific metric:
  covered by the `Requirement`/marker-trait mechanism from the
  Compatibility section above, since `MetricCalculator` carries
  `as_any()`/`requirements()` too.

**`Ranking` carries the computed metrics, with typed access via
downcast:**

```rust
struct Ranking {
    // ...
    metrics: Vec<Box<dyn Metric>>,
}

impl Ranking {
    pub fn metric<M: Metric>(&self) -> Option<&M> {
        self.metrics.iter().find_map(|m| m.as_any().downcast_ref())
    }
}
```

A `Vec`, not a `HashMap<MetricId, _>`, because `MetricId` is a `TypeId`
and so can't itself be serialized (see above) — a `Ranking` needs to
survive a `Competition` snapshot, and each `Box<dyn Metric>` already
carries its own identity via its `typetag` registration once serialized.
The number of metrics on a single `Ranking` is small (a handful at most),
so a linear scan on lookup costs nothing that matters in exchange for not
needing a second, serializable identifier alongside `MetricId`.

A `Ranking` is populated by calling every active `MetricCalculator` for
its phase and inserting each metric it produces.

### Alternatives

- **String-keyed metrics** (`HashMap<String, f64>` or similar). *Advantage:*
  simplest to add a new metric, no new trait needed. *Disadvantage:*
  reintroduces the untyped-bag anti-pattern already rejected for
  compatibility and extra-data above — name collisions and wrong-shape
  values are only caught at runtime. Rejected for the same reason.
- **Representing metrics as marker traits checked via the `Requirement`
  mechanism above**, instead of a dedicated `MetricId`/`Metric` pair (e.g.
  "produces game difference" becomes a marker trait a calculator
  implements, checked with `.cast()` like any other capability).
  *Advantage:* one fewer mechanism to maintain. *Disadvantage:* that
  mechanism answers "does this *instance* implement behavior X", not
  "what *data* did calculating for *this team* produce" — there's no
  per-team value to hand back, and no natural way to express "these two
  active calculators' outputs must be disjoint" through a yes/no
  instance-capability check. Rejected: a different concern (produced data
  identity) from what `Requirement` was built to check (instance
  behavior).
- **Runtime-discovered `produced_metrics`** (call `calculate()` once and
  inspect the resulting keys, instead of a static `produced_metrics()`
  declaration). *Advantage:* one fewer method to keep in sync with
  `calculate()`. *Disadvantage:* validating disjointness would require
  either running calculation with real phase data just to check
  configuration validity, or re-deriving the produced set on every
  ranking recalculation, adding cost to a path that should be free once a
  phase is configured. Rejected in favor of a static declaration checked
  once at `AddPhase` time.

## Tournament progression as an undoable state machine

Tournament progress over time — player registrations, recorded match
results, round advances, and so on — is modeled explicitly as a sequence
of state transitions applied to an initial state, rather than as in-place
mutation of a single `TournamentState`, so that transitions can be undone.

The undo/redo mechanics themselves are generic and don't belong to this
crate: they're provided by the separate `undoable-state-machine` crate in
this workspace (see `undoable-state-machine/DESIGN.md` for its design).
This crate is that library's (initial) consumer, instantiating it as
`StateMachine<TournamentState, TransitionError>`, with transitions
produced by this crate's pluggable axis implementations — e.g. a
`CompetitionMode` deciding to advance a round becomes a
`Transition<TournamentState, TransitionError>`.

## Dependencies this implies

- `intertrait` (and its internal `inventory` dependency) for trait
  casting.
- `typetag` (and its internal `inventory` dependency) plus a serde data
  format of choice for actually writing bytes (not pinned by this
  document) for serializing/deserializing pluggable trait objects.
- No `enum_dispatch`, `linkme`, `erased_serde` (directly — `typetag` uses
  it internally), or `ambassador` in v1 — each was only needed for an
  alternative ruled out above.
