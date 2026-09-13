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

Two complementary mechanisms, chosen based on which requirement applies:

**1. Capability tags, for plain yes/no compatibility.** Core defines a
closed, core-owned vocabulary:

```rust
#[non_exhaustive]
enum Capability { SupportsDraws, RequiresDeterministicPairing, StatefulMatches, /* ... */ }
```

(`#[non_exhaustive]` so core can add variants in a minor version without a
breaking change.) Every axis trait requires:

```rust
fn capabilities(&self) -> HashSet<Capability>; // what it provides
fn requirements(&self) -> HashSet<Capability>;  // what it needs from other axes
```

`TournamentConfig::build() -> Result<Tournament, ConfigError>` collects
capabilities and requirements across all chosen components and checks
`requirements ⊆ union(capabilities)`. This is generic over concrete types —
it only ever looks at capability sets, so it treats third-party and core
implementations identically.

**2. Runtime trait casting, for compatibility needing extra behavior.**
When one component needs to actually call extra methods on another once
compatibility is confirmed (e.g. "if the match outcome also exposes
per-set scores, the ranking system wants to call `get_set_scores()`"),
use the `intertrait` crate:

- Define a marker trait `XYZ` for the richer interface.
- Implementations that support it implement `XYZ` and register the cast
  with `#[cast_to]` (backed by `inventory` internally).
- Consuming code calls `outcome.cast::<dyn XYZ>()`, getting `Option<&dyn
  XYZ>` — `None` means incompatible, `Some` gives a real, compiler-checked
  interface to call.
- Crucially, the base trait (`MatchOutcome`) never needs to know `XYZ`
  exists, so a third party can define a brand-new marker trait and
  register their own type against it entirely within their own crate —
  this is what keeps the mechanism open.

### Alternatives

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
`RankingSystem` implementation should also declare the corresponding
`Capability` (e.g. `Capability::MultiGameOutcome`) in its `requirements()`,
so an incompatible pairing is caught by `TournamentConfig::build()` rather
than discovered as missing data at runtime.

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
- No `enum_dispatch`, `inventory`/`linkme` (used directly), or
  `ambassador` in v1 — each was only needed for an alternative ruled out
  above.
