# Simplification plan

Status: steps 1 to 8 done. The crate does everything it should, i.e. the catalog,
schemas, configs, the compatibility checks, the builder, resolution and the
derive are implemented and tested. This plan reduces it to its core without
losing any of that: first the API a plugin author, a composite author and a UI
see, which matters most, then the implementation behind it.

Nothing outside the crate uses it yet, so every step may break the API freely.
Each step leaves the crate green (`prek run --all-files`), and the tests move
along with it rather than being rewritten at the end.

## What the crate is for

A program that lets someone configure a composite of plugins — pick one of
these, several of those, and answer whatever they ask for — should not have to
know a single plugin. So a plugin describes itself: what to call it, what it
needs to be told, and what it provides to and requires of the composite it ends
up in. Plugins register themselves in a catalog, and a UI reads the catalog and
those descriptions through a builder.

Nothing in the crate knows what a composite *is*. It supplies the vocabulary,
the registry and the checks; a consumer brings the composite and its axis
traits.

## Where the surface comes from

About 45 public items and 8 methods on `Plugin`, for three reasons:

- **Two ways to write a plugin.** Everything a hand-written plugin can get
  wrong — the id written twice, the wrong tagging, a schema drifting from the
  struct — has a runtime check (`check_serialization` and its six
  `SerializationMismatch` variants) and a workaround (`__private::PluginId`).
- **Axes exist twice.** Typed as a generated `FooKind` type with `PluginKind`
  and `PluginObject`, and erased as `Kind`; a user names `MetricKind` where
  they mean `dyn Metric`.
- **Features with several entry points.** Resolution is `resolve`,
  `add_provider_of` and `providers_of` plus `Composite`, `Accepts` and
  `Acceptor`; the check of a finished composite is `FinishError` next to
  `Issue`; the facts of an axis are `Kind::facts` and `facts_of_kind`.

On top, `Plugin` mixes type-level functions (`where Self: Sized`) with methods
on `&self`, which is what `TotalDeclare`, the blanket `Configurable` impl and
the `<P as Plugin>::schema()` ambiguity are working around.

## Target

What using the crate looks like once every step is done:

```rust
// an axis: one attribute, named by its trait from then on
#[plugin_factory::axis]
pub trait Metric: Plugin {
    fn calculate(&self);
}

// a plugin: one derive and one #[register]
#[derive(Debug, Serialize, Deserialize, Plugin)]
#[plugin(
    id = "my_crate::wins",
    label = "wins",
    description = "points per win",
    declare_static = Self::declare_static,
    declare = Self::declare,
    recommends = Self::recommends
)]
struct Wins {
    #[serde(default = "one")]
    #[plugin(label = "Points for a win")]
    for_win: f64,
}

impl Wins {
    fn declare_static(declaration: &mut Declaration) {
        declaration.provide(Points).require::<Matches>("the matches");
    }

    fn declare(&self, declaration: &mut Declaration) {
        if self.for_win > 1.0 {
            declaration.require_that::<EntrantsPerRanking>(
                "the entrants per ranking",
                "rankings of single players",
                |entrants| entrants.0 == 1,
            );
        }
    }
}

#[register]
impl Metric for Wins {
    fn calculate(&self) {}
}

// a composite: one derive
#[derive(Serialize, Deserialize, Configurable)]
struct Phase {
    #[plugin(label = "Format")]
    format: Box<dyn Format>,
    #[serde(default)]
    #[plugin(label = "Metrics")]
    metrics: Vec<Box<dyn Metric>>,
}

// a UI
// renders by walking Phase::schema() along builder.answers(), see step 6
let mut builder = Builder::<Phase>::new(context);
builder.default(&"metrics".parse()?)?;  // what the field offers, recommendations included
builder.candidates(&"metrics[1]".parse()?)?;
builder.set(&"format".parse()?, PluginConfig::of::<Knockout>().with("third_place", true))?;
builder.issues();
let phase = builder.finish()?;           // Result<Phase, Vec<Issue>>, resolves on the way

// a composite built elsewhere, e.g. loaded
let phase = Builder::from_built(context, &phase)?.finish()?;

// at runtime
check(&context, &phase.plugins())?;
let order = order(&phase.plugins())?;
```

The whole public API, about 25 items:

- **Defining plugins:** `#[axis]`, `#[register]`, `#[derive(Plugin)]`,
  `#[derive(Configurable)]`; `Plugin` (`declare`, `recommends`), `PluginType`
  (`ID`, `LABEL`, `DESCRIPTION`, `declare_static`), `Configurable` (`schema`,
  `plugins`), `FieldType` (`kind`, `missing`, `plugins`).
- **Facts:** `Declaration` (`new`, `of`, `provide`, `require`,
  `require_that`), write-only for a user; `Incompatibility`
  (`FactProvidedTwice`, `FactProvidedByContext`, `FactNotProvided`,
  `ConditionNotMet`, `CyclicRequirements`); `check(context, plugins)`,
  `order(plugins)`.
- **Catalog:** `Axis` on `dyn Trait` (`entries`, `entry`), `AnyAxis` (`of`,
  `entries`, `entry`), `PluginEntry` (`id`, `label`, `description`, `schema`).
- **Schemas:** `Schema` (`fields`, `field_named`), `Field` (`name`, `label`,
  `description`, `kind`, `default_value`), `FieldKind` (`Bool`, `Integer`,
  `Decimal`, `Plugin(AnyAxis)`, `List`, `Optional`), all read-only.
- **Configs:** `Value`; `Config` (`new`, `with`, `get`); `PluginConfig`
  (`of`, `new`, `with`, `plugin`, `config`, `build`); `Path` (`ROOT`, `field`,
  `index`, `segments`, `Display`, `FromStr`) and its `ParsePathError`;
  `Segment` (`Field`, `Index`), to read a path; `ConfigError` (`UnknownField`, `Unanswered`, `WrongKind`,
  `UnknownPlugin`, `Rejected`, `path`).
- **Builder:** `Builder` (`new`, `from_built`, `answers`, `default`,
  `candidates`, `issues`, `set`, `unset`, `finish`), `Candidate` (`entry`,
  `conflict`), `Issue` (`path`, `problem`), `Problem` (`Invalid`,
  `Incompatible`, `Unprovided`).

This is deliberately the smallest API that keeps the functionality; anything
a consumer turns out to need — reading declarations, writing a schema by hand —
is added back when it does.

## Vocabulary

- **plugin** — a type implementing `PluginType` (and so `Plugin` and
  `Configurable`) and one axis trait. Usually derived.
- **axis** — a plugin trait together with the registry its plugins register
  in, declared with `#[axis]`. Typed it is `dyn Trait`, which implements
  `Axis`; erased it is an `AnyAxis`. The word *kind* is reserved for
  `FieldKind`, the kind of answer a field takes.
- **composite** — what plugins are configured into: a `Configurable` that is
  not a plugin, derived. Its fields are its **slots**.
- **context**, **fact**, **declaration** (static and configured), **open**,
  **schema**, **config** — as before: facts are `'static + Debug + Sync` types,
  the context is what holds before any plugin is chosen and cannot be provided
  again, the static declaration holds for every configuration and is readable
  from the catalog, the configured one adds to it.

## Steps

### 1. Split `Plugin` by what it is asked on

`Plugin` keeps only what a built plugin behind `dyn` is asked; everything about
the type moves to `PluginType`, and the schema to `Configurable` alone:

```rust
pub trait Plugin: Debug + __private::Erased + 'static {
    /// What this configuration provides and requires on top of the static declaration.
    fn declare(&self, _: &mut Declaration) {}
    fn recommends(&self) -> Vec<PluginConfig> { Vec::new() }
}

pub trait Configurable: Sized + DeserializeOwned + 'static {
    fn schema() -> Schema;
    /// The plugins held by the fields, in schema order.
    fn plugins(&self) -> Vec<&dyn Plugin> { Vec::new() }
}

pub trait PluginType: Plugin + Configurable {
    const ID: &'static str;
    const LABEL: &'static str;
    const DESCRIPTION: &'static str;
    fn declare_static(_: &mut Declaration) {}
}
```

`__private::Erased` is implemented for every `PluginType` by a blanket impl
(the cycle `Plugin: Erased`, `impl<T: PluginType> Erased for T` is the one
`TotalDeclare` relies on today). It gives a `&dyn Plugin` what only the type
knows: `id()`, `declare_all()` (static, configured, then the sub-plugins'
recursively) and `sub_plugins()`, forwarded to `Configurable::plugins`. So
`sub_plugins` is never written by hand, and neither is the forwarding a
composite plugin needed.

- Remove `TotalDeclare`, `__private::PluginId`, the blanket `Configurable`
  impl, `Plugin::schema`, `Plugin::sub_plugins` and every `where Self: Sized`.
- `PluginEntry` stores `&'static str` for id, label and description instead of
  function pointers.
- `#[register]` names the typetag tag `<P as PluginType>::ID`, so a plugin
  written by hand also writes its id once.
- Derive: emits `Plugin`, `Configurable` and `PluginType`. The attributes
  become `declare_static`, `declare` and `recommends`. `plugins` is generated
  from the fields through `FieldType::plugins`, as `sub_plugins` is today.
- `Plugin: Debug` settles the open question of where `Debug` belongs: the
  checks name plugins by it.

### 2. Name an axis by its trait object

`#[axis]` on the trait replaces `plugin_kind!`:

- It emits `#[::typetag::serde(tag = "type", content = "value")]` on the trait,
  so the adjacent tagging the crate depends on can no longer be got wrong; the
  trait must not carry its own `typetag` attribute.
- It emits a hidden registry type, `inventory::collect!` for it, and
  `impl Axis for dyn Trait`. `Axis` replaces `PluginKind` and `PluginObject`:
  `entries()`, `entry(id)`, and the hidden constructor of a registry value
  that `#[register]` submits.
- `Axis: Plugin` as a bound of the impl makes a trait without the `Plugin`
  supertrait fail to compile at the attribute.
- `AnyAxis` (today's `Kind`) is the erased value `FieldKind::Plugin` holds,
  `AnyAxis::of::<dyn Metric>()`.
- Every `K: PluginKind` parameter becomes `A: Axis + ?Sized`:
  `PluginConfig::build::<dyn Metric>()`, `FieldType for Box<A>`.
- Generic traits are rejected by the attribute.

### 3. Register through `#[register]` only

With the id a constant and the tagging generated, the mismatches
`check_serialization` looks for can no longer arise for a registered plugin.

- `PluginEntry::new` and the registry constructor become `#[doc(hidden)]`;
  `#[register]` is the one way into the catalog, for derived and hand-written
  plugins alike.
- Each axis builds a map from id to entry on first access (`OnceLock` in the
  hidden registry), which makes `entry(id)` a lookup instead of a scan, and
  panics there if two entries share an id, naming the id and both types
  (`std::any::type_name`, stored in the entry).
- Remove `check_serialization` and `SerializationMismatch`. A schema can no
  longer drift from its struct, since `Configurable` is only derived (step 4).
- Unit structs: the derive knows from the syntax whether a plugin is a unit
  struct and sets a hidden `Configurable::IS_UNIT` constant, replacing
  `deserializes_from_null` probing at runtime. It sits on `Configurable`
  rather than `PluginType` because `Configurable` is the impl that is always
  derived (step 4), so a plugin with a hand-written `PluginType` still gets it
  right.
- The entries of an axis come in the order of their ids, which the builder's
  candidates and providers relied on sorting for.

### 4. Derive `Configurable` for a composite

`#[derive(Configurable)]` is the schema half of `#[derive(Plugin)]`: schema and
`plugins()` from the fields, with the same serde rules and the same field
attributes. A composite then writes no `Schema` by hand, and its `plugins()` is
what the runtime checks take (step 5).

`Configurable` is only derived, for plugins and composites alike; implementing
it by hand is unsupported. So schemas become read-only for a user:

- `Field::new`, `described`, `with_default` and `optional` become
  `#[doc(hidden)]`, for the derive alone; `Field::bool`, `integer`, `decimal`,
  `plugin`, `plugins`, `repeated` and `FieldKind::plugins` are removed, and with
  `repeated` its wrapping of a default into a one-element list.
- `Schema::new` and `Schema::field` become `#[doc(hidden)]`; `Schema::complete`
  becomes crate-internal.
- `FieldKind`'s constructors `list` and `optional` stay, as a `FieldType` impl
  of a newtype may need them.

### 5. Check a built composite through its plugins

The consumer's runtime API becomes two functions over `&[&dyn Plugin]`, naming
plugins by `Debug`:

- `check(context: &Declaration, plugins: &[&dyn Plugin]) -> Result<(), Incompatibility>`,
  today's `check_facts` with nothing open.
- `order(plugins: &[&dyn Plugin]) -> Result<Vec<usize>, Incompatibility>`.

What only the builder and the catalog need becomes crate-internal:
`declaration_of`, `unprovided`, `check_addition`, `check_facts` with `open`,
`OpenFacts`, and the `(String, Declaration)` tuples, which become an internal
named struct.

A user only writes declarations; why plugins do not fit is told by
`Incompatibility`, `Candidate::conflict` and `Problem`. So:

- `Declaration` keeps `new`, `of`, `provide`, `require` and `require_that`
  (step 7). `provides_type`, `provided`, `required`, `ProvidedFact` and
  `RequiredFact` become crate-internal.
- `PluginEntry::declaration`, `declaration_for` and `complete` become
  crate-internal.
- `Fact` becomes `__private`: it is implemented for every
  `'static + Debug + Sync` type and only appeared in the API through
  `provided()`. The bounds of `provide`/`require` spell it out.
- `Incompatibility::OutOfOrder` goes; nothing constructs it.

### 6. Resolve and finish in the builder

Resolution moves onto the answer tree, which removes `resolve`,
`add_provider_of`, `providers_of`, `Composite`, `Accepts` and `Acceptor`:

- The builder knows the list fields of the root schema and their axes. A fact
  required, provided neither by a plugin nor by the context, is settled iff
  exactly one plugin needing no configuration, across the axes of those
  fields, provides it, and its axis is taken by exactly one root list field
  — the rules of today's `resolve`, with "accepts" read from the schema: a
  single slot is never added to, an axis in two list fields is never resolved.
- Only root-level fields count, as `Accepts` was implemented by the composite
  alone; a list inside a plugin (`Combined::parts`) is never added to.
- `Question` and `Builder::questions` go. A UI renders from the schema: for
  each field of `T::schema()` it reads the answer from `builder.answers()`,
  and for a plugin answer it recurses into
  `field_axis.entry(id).schema()`, building the `Path` with `field` and
  `index` as it goes.
  That walk is documented on `Builder` with an example. The builder keeps it
  internally (`Tree`), for open facts, for where an `Unprovided` issue goes
  and for the order of defaults in `finish`; what the schema cannot tell a UI
  is a field's actual default, which stays `Builder::default(path)`.
- `finish` on a copy of the answers: accept the defaults depth first in schema
  order,
  resolve, repeat until nothing changes, then report. It returns
  `Result<T, Vec<Issue>>`, the issues being those of `issues()` with nothing
  open. `FinishError` goes.
- `Builder::from_built(context, &composite)` (`T: Serialize`) starts from a
  built composite's answers, which are what it serializes to, so a composite
  loaded or built elsewhere is resolved and checked by `from_built(..).finish()`.

### 7. Smaller things a user writes

- `require::<T>(name)` without a condition, and
  `require_that::<T>(name, description, holds)` with one; `Condition` goes.
- `PluginConfig::of::<P>()` and `PluginConfig::new(id)` answer nothing;
  `.with(field, answer)` answers a field, as on `Config`.
- `impl FromStr for Path`, the inverse of its `Display`
  (`"metrics[1].for_win"`); the tests' own `path` helpers go.
- `Path::field(name)` and `Path::index(position)` extend a path;
  `Path::of`, `then` and `starts_with` become crate-internal, and `Segment`
  is only read, through `Path::segments`.
- `Config` keeps `new`, `with` and `get`; `set`, `unset` and `answers` become
  crate-internal, since the builder holds the answers being edited.
- Remove `facts_of_axis`; `AnyAxis::facts` and `PluginEntry::sub_axes` become
  crate-internal.
- Remove `Candidate::fits`, which is `conflict().is_none()`.
- The `inventory` re-export moves to `__private`; only generated code uses it.
- The derive's `#[plugin(description = ..)]` on the struct becomes optional, as
  it is on a field.

### 8. Implementation

None of this changes the API.

- **One recursion over answers.** `Schema::complete`/`check`,
  `FieldKind::complete`/`check`/`map_plugins`, `Kind::complete`/`check` and
  `PluginEntry::complete` become one walk with a `fill_defaults` flag.
- **Paths are built on the way down.** Pass the current `Path` into the walk
  and create errors with their full path; `ConfigError::within`,
  `Path::prefix`, `prepend` and `parent` go.
- **Build each placed plugin once.** `Tree` stores
  `Result<Box<dyn Plugin>, ConfigError>` per placed plugin; `issues`, defaults
  and declarations read it instead of calling `instance` again, and
  `Placed::declaration`/`configured_declaration` merge. `finish` stops
  rebuilding every default for every remaining unanswered field.
- **Cache the facts of an axis.** The transitive closure `AnyAxis::facts`
  computes is constant for a program; keep it in the registry's `OnceLock`.
  `candidates` asks for it per entry and sub-axis.
- **`issues` reuses the check.** Run the internal check once per plugin and
  route `FactNotProvided` to the fields that could provide the fact, instead of
  the separate unprovided pass.
- **One way to deserialize.** Drop the `is_config` flag of
  `Path::of_deserialized` if deserializing everything in its tagged form keeps
  the paths right; otherwise leave it.

### Tests and docs, per step

- Hand-written test plugins in `catalog.rs`, `schema.rs`, `builder.rs` and
  `compatibility.rs` become derived where the test is not about writing one by
  hand; one plugin with hand-written `Plugin` and `PluginType` impls (and a
  derived `Configurable`) stays, covering step 1's claim that it writes its id
  once. Tests of the crate itself may still build a `Schema` through the
  hidden constructors where a hand-made schema is the point, e.g. a default
  of the wrong kind.
- `tests/workflow.rs` follows the target above: derived composite,
  `finish` resolving, `from_built`, `check`/`order` on `plugins()`, and the
  documented schema walk in place of `questions()`.
- `tests/derive.rs` gains `Configurable` and loses `check_serialization`.
- Compile-fail cases (missing `Plugin` supertrait on an axis, a typetag
  attribute next to `#[axis]`, a generic axis) are unit tests of the macro
  expansion, as the derive's are now.
- The crate doc example in `lib.rs` and the doc of each changed item are
  rewritten with the step that changes them.

## Open questions

- **Should `issues()` report what `finish` would resolve?** Reporting it keeps
  "nothing is answered behind the operator's back"; not reporting it is
  consistent with `finish` accepting defaults. Leaning towards reporting it as
  `Unprovided` with the single provider, which is what a UI would set anyway.
- **The derive names the crate `::plugin_factory`**; a renamed dependency
  breaks it. `#[plugin(crate = "..")]` as serde does, if needed.
- **A struct deserialized through `try_from` cannot be derived.** The schema
  would have to come from the type it is converted from.
- **Recommendation conflicts.** First in schema order wins; weights only if a
  real case needs them.
- **Who owns the UI text.** `&'static str` means one language; localisation
  would wrap it, keyed by entry id and field path.
- **Conditions cannot capture**, being `fn(&T) -> bool`. Fine so far.

## Decided

- **No negative compatibility.** A plugin never declares what it is
  incompatible *with*.
- **The context is immutable.** A plugin may not provide a fact the context
  provides, so every provided fact is settled.
- **Optimism about open slots only.** Hiding a plugin that would have worked is
  the expensive mistake; the strict check stays the authority.
- **The static/configured split of a declaration stays.** It is what makes
  filtering candidates before they are configured sound, so it is the core and
  not a simplification target; only its names change.
- **No untyped bags.** A plugin is built as `Box<dyn Axis>`, deserialized at a
  position whose axis is known; nothing is downcast.
- **A plugin is identified by a namespaced string id**, a persistence contract
  like a `typetag` tag. `TypeId` stays for facts and axes.
- **Persistence stays with the typed plugin**, through its serde impl and
  `typetag`, so non-self-describing formats keep working; the answer tree is
  JSON between a UI and the catalog.
- **The crate does not reimplement `typetag`.** `#[axis]` and `#[register]`
  emit `typetag` attributes; the crate keeps no tag registry of its own.
- **The smallest API first.** A user writes declarations and reads schemas
  but neither reads declarations nor writes schemas, and `Configurable` is
  only derived. Each of these is added back when a consumer needs it, not
  before.
- **One way into the catalog.** `#[register]`, for derived and hand-written
  plugins; this is what makes the serialization checks unnecessary.
- **A composite says what may be added to it through its schema**: a root
  list field of an axis. A single slot is never added to.

## Not in this crate

Porting a consumer onto it, its axis traits, its composite and its UI. A step
is finished when its items are implemented and tested against axes of the
crate's own.

The crate stands on its own so that it can be moved out of the workspace: its
`Cargo.toml` declares version, edition, license and dependency versions itself,
it depends on no crate of the workspace but `plugin-factory-derive`, which
moves with it and is set up the same way, and every test uses axes and plugins
of its own.
