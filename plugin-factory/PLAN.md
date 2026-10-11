# Development plan

Status: the compatibility layer, the catalog and the schemas are implemented, the
builder is not. Everything here is about this crate alone; what a consumer does
with it is out of scope, see [Not in this crate](#not-in-this-crate).

## What the crate is for

A program that lets someone configure a composite of plugins — pick one of
these, several of those, and answer whatever they ask for — should not have to
know a single plugin. So a plugin describes itself: what to call it, what it
needs to be told, and what it provides to and requires of the composite it ends
up in. Plugins register themselves in a catalog, and a UI reads the catalog and
those descriptions.

Nothing in the crate knows what a composite *is*. It supplies the vocabulary,
the registry and the checks; a consumer brings the composite and its plugin
traits.

## Vocabulary

- **plugin** — a type implementing [`Plugin`](src/lib.rs) and one axis trait.
- **kind** (axis) — a plugin trait together with the registry its plugins
  register in, declared with `plugin_kind!`. A kind is `K`, its trait object
  `K::Object`.
- **composite** — what plugins are configured into. It has **slots**, each
  taking one plugin or several of one kind.
- **context** — what is already true of the composite before any plugin is
  chosen, and what no plugin can change: providing a fact the context provides
  is an error, as providing one another plugin provides is.
- **fact** — a `'static + Debug + Sync` type, optionally with a value, that
  plugins provide and require. Facts are the only thing compatibility is decided
  on.
- **declaration** — the facts one plugin provides and requires. The **static**
  part holds for every configuration of the plugin and is readable from the
  catalog before it is configured; the **dynamic** part adds what this
  configuration provides and requires on top, and the values.
- **open** — a fact type that a slot still to be filled could provide, so that a
  requirement of it that nothing provides yet is not decided.
- **schema** — the fields one plugin or composite has to be told, each taking an
  answer of one kind and, unless it has to be answered, offering a default. The
  answers to it are a **config**, keyed by field name.

## Implemented

- `compatibility`: `Fact`, `Declaration`, `Condition`,
  `ProvidedFact`/`RequiredFact`; `check_facts(context, declarations, open)` and
  `check_addition(…, addition, open)`; `order`, `unprovided`, `declaration_of`;
  `OpenFacts` with `NOTHING`.
- `catalog`: `PluginKind` (with `entry(id)`) and `plugin_kind!`;
  `PluginEntry<K>` holding `id`, `label`, `description`, `declare`, `schema`
  and `build`, all as const-storable function pointers; `Kind`, a kind as a
  value, which a field taking plugins holds so that a nested config can be
  completed against the schema of its plugin; `Accepts<K>`, `providers_of`,
  `add_provider_of`, `facts_of_kind`; `check_serialization`, see
  [Identity and persistence](#identity-and-persistence).
- `Plugin` with
  `id`/`label`/`description`/`static_declare`/`dyn_declare`/`sub_plugins`, and
  `TotalDeclare`, which walks the sub-plugins so a composite plugin forwards
  nothing by hand.
- `schema`: `Configurable: DeserializeOwned` with `schema()` and a defaulted
  `recommends()`; `Schema`/`Field`/`FieldKind`. A field takes yes or no, a whole
  number, a number, one plugin of a kind, a list of answers of one kind
  (`List`, so that "several plugins of a kind" is a list of plugins) or an answer
  of one kind or nothing (`Optional`, answered by the answer or by `null`), the
  last two nesting as deep as they like; `Field::optional`/`Field::repeated` wrap
  what a field takes, and a list its default with it. `Schema::complete` answers
  every unanswered field by the default of the field, recursing into the config
  of every plugin answer, which is what makes "needs no configuration" mean
  "every field has a default", and checks answers and defaults alike against the
  kind of their field. `PluginEntry::build` completes a config and deserializes
  the plugin from it.
- `config`: `Config`, a JSON object of answers keyed by field name, each a
  `serde_json::Value`; `PluginConfig`, a plugin answer, written
  `{"type": id, "value": config}`; `ConfigError` with the `Path` of what it is
  about, which a nested error is prefixed with on its way out, so that an answer
  of the wrong kind inside a list is reported as `weights[1]`. What a plugin's
  `Deserialize` impl rejects is `ConfigError::Rejected`, its path taken from
  `serde_path_to_error` with the `type`/`value` wrapping of plugin answers left
  out. A whole number answers a decimal field.

### Identity and persistence

A plugin is a serde type. It is built by deserializing its completed config, so
its `Deserialize` impl is the only place that reads answers, and validation
lives there too (`deserialize_with`, `try_from`), which also rejects an invalid
saved plugin. A nested plugin field is a `Box<dyn K>` and deserializes through
the consumer's `typetag` impl, so a config is exactly what `typetag` writes
for a plugin tagged adjacently with `tag = "type", content = "value"`: a built
plugin serializes as the completed config it was built from. A plugin without
parameters may be a unit struct: the entry checks whether the type
deserializes from `null`, and if so its completed config is written as `null`,
as serde writes a unit struct.

The id (`Plugin::id`) is therefore written twice, once as the id and once as
the `typetag` name, since `typetag` exposes a name neither by type nor through
a public registry. `check_serialization::<K>()` checks the two against each
other, plus unique ids and that every schema field is serialized under its
name; a consumer calls it in a test per kind. It only serializes plugins that
need no configuration. The derive macro (step 5) removes the duplication by
emitting both from one attribute.

## Steps

Each one is shippable on its own.

### 1. Erased kinds

`Kind` (`Kind::of::<K>()`) exists already, holding the kind's `TypeId` and how
to find the schema of one of its plugins. Still to come is `EntryView`, the object-safe view of an entry a UI browses through — label,
description, static declaration, schema, and building an instance *into* a
composite, so that construction stays typed and nothing is downcast.

With `Kind` in place, an entry can also report the kinds its own plugin fields
take, which is what makes openness transitive: a slot still to be filled can be
filled with a plugin that has slots of its own. Until then `facts_of_kind`
covers one level and the gap is a documented approximation.

### 2. The builder

`Builder<T>` over one answer tree, addressed by the `Path` a config error
carries, and the queries a UI renders from: `questions()`, `default(path)`,
`candidates(path)`, `issues()`, plus `set` and `finish`. `Candidate` says "fits"
or "conflicts, because …"; `Issue` is something currently wrong, anchored to the
field where it gets fixed.

The rules the queries follow, which the compatibility layer already supports:

- **A field is answered or not.** An answered field is given, whether it holds
  one plugin or several; an unanswered one is a wildcard, and its kind's facts
  are open. Accepting a default is answering, so nothing is ever recomputed
  behind an operator's back.
- **Candidates are per position.** For a candidate at some path, the plugins
  occupying that path are left out and the candidate takes their place, so a
  single slot does not conflict with what it replaces and a list does not
  conflict with itself.
- **A candidate conflicts iff `check_addition` says so**, with `open` computed
  from the fields still unanswered. The filter is therefore the strict check
  weakened by one argument, and `OpenFacts::NOTHING` at `finish` cannot let an
  invalid composite through.
- **Defaults come from the plugin or from its siblings' recommendations**, never
  from a table in the consumer.

### 3. A generic resolution loop

Today a consumer writes the loop itself: `unprovided` → `add_provider_of` per
kind → repeat, since an added plugin can require facts of its own → `order`.
Lifting it here needs a composite to be describable (its declarations, and which
kinds it `Accepts`), which step 1's `Kind` makes possible. Optional: the loop is
six lines, and a consumer that wants a different policy should be able to write
its own.

### 4. Enforce the static/dynamic contract

The filter reads an entry's **static** declaration, so it is sound only if, for
every configuration of the plugin:

- every fact type it provides is declared statically — otherwise a candidate is
  hidden whose conflict would only have been real for some configurations, or
  openness misses a provider;
- every fact it requires *unconditionally* is declared statically, and anything
  conditional is declared dynamically — more in the static part means more
  filtering, which stays sound only while it is true of every configuration.

The first half is checkable: when a declaration is taken from an instance,
assert that it provides no fact type the static declaration did not. One
assertion where `declaration_of` is used, off any hot path. The second half
stays a documented contract.

### 5. A derive macro

For the registration boilerplate — `id`, `label`, `description`, and the
schema of a plugin derived from its fields and their serde attributes, so that
field names and defaults are not written next to the struct a second time —
once the entry shape has stopped moving. It also emits the `typetag` name from
the id, so that the id is written once. Separate crate.

## Open questions

- **Schema and struct can drift.** Until step 5, a field's name and default are
  written in the schema and the struct. `check_serialization` catches a schema
  field the struct does not serialize; a field the struct has and the schema
  lacks fails at build as a missing field.
- **Who owns the UI text.** `&'static str` from the plugin means the crate ships
  one language; localisation would wrap it, keyed by entry id.
- **Recommendation conflicts.** Two configured plugins recommending different
  things for one single-plugin field: first in schema order wins, or do
  recommendations need a weight?
- **Should a candidate distinguish "fits" from "fits only if some open slot
  provides X"**, so that a UI can sort rather than treat all candidates alike?
- **Where `Debug` belongs.** The checks name plugins by their `Debug` form, and
  `declaration_of` asks for it explicitly rather than `Plugin` requiring it, so
  each axis decides. Simpler would be `Plugin: Debug`.
- **Conditions cannot capture**, being `fn(&T) -> bool`, which is what keeps
  `require::<F>(name, None)` inferable. Fine so far.

## Decided

- **No negative compatibility.** A plugin never declares what it is incompatible
  *with*. Everything a filter needs beyond the positive facts is the one set of
  open fact types.
- **The context is immutable.** A plugin may not provide a fact its composite's
  context provides, which makes every provided fact settled: `open` only ever
  answers whether a fact *nothing* provides yet could still arrive, and a
  condition on a provided fact is decided at once however the composite is
  filled up. A plugin that would change what its own composite stands for has to
  be a context of its own instead.
- **Optimism about open slots only.** Hiding a plugin that would have worked is
  the expensive mistake; offering one that later turns out not to fit costs a
  revised answer. The strict check stays the authority.
- **No untyped bags.** A kind's constructor returns `Box<K::Object>`, so nothing
  is downcast; one global registry returning `Box<dyn Any>` is rejected for that
  reason.
- **A plugin is identified by a namespaced string id**, a persistence contract
  like a `typetag` name. `TypeId` stays for facts and kinds, which never leave
  the process.
- **Persistence stays with the typed plugin, not the answer tree.** A plugin is
  saved by its own serde impl through `typetag`, so non-self-describing formats
  such as postcard keep working; a `serde_json::Value` inside saved data would
  need `deserialize_any`. The answer tree is JSON and lives between a UI and the
  catalog, though it happens to be what `typetag` writes to JSON.
- **The crate does not reimplement `typetag`.** The catalog deserializes the
  concrete type it registered, so it needs no tag registry of its own; the cost
  is the id written twice until step 5.
- **A plugin does not add itself to a composite.** `Accepts<K>` sits on the
  composite, so "this slot takes one plugin and nothing may be added to it" is
  expressible by leaving the impl out.

## Not in this crate

Porting a consumer onto it, its plugin traits, its composite and its UI. The
crate is finished for a given step when the step's items are implemented and
tested against an axis of its own, i.e. without depending on which plugins a
program happens to link in.

The crate stands on its own so that it can be moved out of the workspace it
currently sits in: its `Cargo.toml` declares its version, edition, license and
dependency versions itself rather than inheriting them, it depends on no crate
of that workspace, and every test uses an axis and plugins of its own.
