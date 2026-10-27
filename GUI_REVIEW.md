# GUI review (2026-10-26)

Exploratory end-to-end review of `tournament-gui` (release build, headless Chrome through
`tournament-gui-e2e`) by four testers: setup and phase configuration, playing non-knockout formats,
knockout brackets, cross-cutting concerns (sizes, themes, language, tabs, storage). Sizes 360×740,
390×844, 768×1024, 820×1180, 1024×768, 1280×900, 1920×1080; light and dark; English and German;
2–40 entrants. Screenshots were left in `/tmp/gui-review/{setup,playing,bracket,crosscut}/`.

No page logged an error, and axe-core found no violation at the end of any run. Every standing and
every bracket advancement checked by hand was right, including after undo, redo and reopen.
Two tabs (read-only, catching up, taking over), reload, export/import and offline use worked.

Severity: **H** high, **M** medium, **L** low. Ids are referred to by the plan below.

## Findings

### Engine and app logic

| Id | Sev | Finding |
|----|-----|---------|
| E1 | H | **Byes score before their round.** Round robin, random rounds and Monster DYP schedule every round at once; a bye is final when scheduled, so `update_after_bye` runs at the start. 3 entrants: all on 1 point at 0-0-0. 27 entrants: a loser of one match is tied 2nd. Buchholz and Sonneborn-Berger inherit it. (`competition.rs` ~l. 450, DESIGN.md *Byes*.) |
| E2 | M | Editing the rounds of a random-rounds phase after the start redraws round 1's pairings; after undo the pairings return but numbers and sides differ. *To confirm*: whether the rng depends on the config. |
| E3 | M | Reopening a result is offered (`Reopen` enabled) where it is refused ("a later result depends on this one"), and the refusal doesn't name the later match. |
| E4 | M | Bracket draws pairings that will never be played: the reset after the winner bracket's champion wins the grand final; "Bye vs Bye" / "Bye vs Loser of 1" in a loser bracket; a third-place pairing "Loser of 1 vs Bye" with 3 entrants (the list says "Bye: Alice (Third place)"). (`tournament-app/src/session/bracket.rs`, possibly the formats.) |
| E5 | L | "(if needed)" stays on the reset after it was scheduled and played (`only_if_needed` looks only at where the sides come from). |
| E6 | M | Metric values are not translated: "round 1" in the standings in German (`round_of_first_loss.rs`, `round_of_elimination.rs` `MetricColumn` Display), and `Score`s show as `0.5` in German. |

### Phase form

| Id | Sev | Finding |
|----|-----|---------|
| F1 | H | **Adding a phase with issues shows a raw internal error**: "the phase cannot be played this way: metrics: ranking[0] requires tournament_engine::plugins::metrics_calculator::points::Points …", at the top of the page (often out of view), also in the edit dialog, and it stays up after later changes. The incompatibility comes through as `Rejected` (`error.rs` `rejected`). |
| F2 | M | **Undoing "Add phase" resets the form** to round robin with its defaults instead of the configuration just undone (also note 1 of `improvements.txt`). |
| F3 | M | A format change drops a metric that a criterion the user kept still needs (e.g. *more points* kept, *points* dropped by single elimination), producing an issue and then F1. Switching to random rounds after a manual ranking edit ticks Buchholz metrics that no criterion uses. (`form.rs`, "later fields follow".) |
| F4 | M | Enter in a number field submits the form and adds the phase. |
| F5 | M | Number messages: `2.5` in an integer field says "“2.5” is no number"; `abc`, empty and `1e400` say "“” is no number" (the browser empties `type=number`); `1e3`, `99999999999999999999` are "no number"; `0,5` is silently taken as **5**. |
| F6 | M | Metrics reorder when (un)ticked (checked ones first), so a second click at the same spot hits another metric. |
| F7 | L | A phase with no ranking and no metrics is accepted without a warning; everyone ties forever. |
| F8 | L | Start is disabled with 0–1 entrants without saying why. |
| F9 | L | Tabbing out of a number field opens the next toggletip over the field's issue line. |

### Playing (list, typed results)

| Id | Sev | Finding |
|----|-----|---------|
| P1 | M | Errors of the typed result entry show in the page-wide alert at the top, pushing the layout down (~56px) and far from the input (on a phone two screens away); the input gets no `aria-invalid`/`aria-describedby`; the error stays while stepping rounds. The same placement problem holds for every error and announcement (also F1). |
| P2 | L | Typed entry: `1x` refused (only `1 x`); "the match has a result already" doesn't name the match; `0 1` says "there is no match 0" but `-1 1` gives the generic message; straight vs curly quotes. |
| P3 | L | Searching for the entrant with the round's bye shows "No match here has an entrant whose name contains …" right above "Bye: …" (`play.rs`, the `rows.is_empty()` branch ignores byes). |
| P4 | L | Undo walks past the start back into setup without any warning. |

### Bracket view

| Id | Sev | Finding |
|----|-----|---------|
| B1 | H | **Lines don't reach pairings narrower than their column**: a pairing is as wide as its text, a line starts at the column's edge. Any name longer than ~10rem in a column leaves the others' lines floating. |
| B2 | H | **Names never wrap in the bracket** (`width: max-content`), so on a phone they are cut by the scroller ("Maximilian-Alexander Müller-Lüder…"), against *names are never cut*. Just wrapping breaks the equal-slot geometry (lines miss by 6–18px). |
| B3 | M | Nothing shows that an open name is a button that reports a win (only a hover tint, absent on touch). |
| B4 | M | Bye pairings look like open ones (solid), and "Bye" is styled like the pending "Winner of 1". |
| B5 | L | Placeholders are long and uneven: "Winner of Winners round 2 (4)" next to "Winner of 13"; "Winner of 8th-finals (5)". English stage names "8th-finals"/"16th-finals" should be "Round of 16"/"Round of 32"; "Winner bracket" heading vs "Winners round n". |
| B6 | L | The chosen view (bracket or list) is forgotten on reload. |
| B7 | L | 16 entrants at 820px overflow the scroll area by 7px (a needless scrollbar). |
| B8 | L | Phone: three sections of a double elimination scroll sideways each on its own; nothing hints that more columns follow; a focus move after a report scrolls sideways and down at once. |

### Layout, theme, language

| Id | Sev | Finding |
|----|-----|---------|
| L1 | H | **On phones the whole page scrolls sideways once the standings are shown** (598px wide in a 375px viewport): below 60rem `.columns` has no `grid-template-columns`, so the implicit `auto` track grows to the table's min-content and the table's own scroller never scrolls. The phase summary's `dl` overflows as well. |
| L2 | M | At 768px the setup's entrant column is ~255px, minus "Remove" ~90px: names break inside words ("Maximilia/n-/Alexande/r"). |
| L3 | M | German, 640–750px: the competition name in the bar is 0px wide (the bar's labels collapse to icons only below 40rem). |
| L4 | L | The edit dialog on a phone centres all its text; Save/Cancel only after scrolling to the end (on desktop too); the backdrop barely dims in dark mode; the page behind gets a horizontal scrollbar. |
| L5 | L | `document.title` carries the isolation marks U+2068/U+2069 (`main.rs` ~l. 204). |
| L6 | L | A bad import file says "the file is no competition this version can open: expected ident at line 1 column 2" (serde's raw message, lowercase). |
| L7 | L | DESIGN.md (*Layout*) says buttons choose the setup card below 60rem; the code (`competition.rs` ~l. 300) stacks both. |
| L8 | L | Focus falls to `<body>` after toggling a metric, ↑/↓, Remove, Add phase, and on Previous/Next round, Undo/Redo when the button becomes disabled; after a plugin select it stays on the hidden listbox. |
| L9 | L | Competition names: duplicates can't be told apart in the list (also after importing a copy); a long name is cut with "…" in the header with no way to see it whole. |
| L10 | L | Entrant names: case-only duplicates (`Alice`/`alice`) and NFC-different equal-looking names are accepted. |

### Look and feel

- **No finished state.** After the last result there is just the last round and "Playable now (0)": no
  winner, nothing for a tie for first, and the list of competitions says "Every match played" without
  the winner. "Edit phase" stays prominent.
- **Feedback far from its cause.** Errors and announcements always appear above the cards; an empty
  ~40–50px slot is kept for them on every screen.
- **Scrolling.** Match cards are ~100px, a 20-match round ~2000px, finished matches stay on top; the
  bar (Undo, view switch) is not sticky; on phones the storage warning takes ~250px of the first
  screen (with the read-only notice ~70%); the entrant list is an inner scroll box (`max-height:
  32rem`) inside the page's scroll, and a new entrant at its end stays out of view.
- **What looks like a button.** "Remove", "Reopen" look like text; ↑/↓ are tiny low-contrast
  glyphs; the bracket's clickable names look like plain text (B3). Touch targets are below 44px.
- **Hierarchy.** "Add phase" (right-aligned primary) and "Start" (left-aligned primary below)
  compete; "Edit phase" and "Start" touch with no gap. The format row has two ⓘ (the field's and the
  format's) with nothing telling them apart. Range hints "(a number of at least 0)" after every input
  are noisy, wrap mid-phrase on phones, and misalign the inputs.
- **Standings.** Half-width on desktop: tie-break headers are cut ("Sonneborn-B…", "Buchholz of
  Buchholz" hidden) while the matches have room; the name column wraps while Sonneborn-Berger keeps
  ~170px empty. Nothing tells a 0-point loser from someone who hasn't played.
- **Wording.** Labels mix case ("points", "Grand final reset", "Buchholz"); errors are lowercase
  fragments; the metrics tip says "team" where the page says "entrant"; "Points for a draw" is offered
  for knockouts that have no draws; "Round 1" as a heading repeats the select above it.
- **Bracket.** Small brackets use a third of a laptop card; a 32-entrant bracket is ~1100px tall with
  its column headings scrolled away; the double elimination's sections aren't visibly joined to the
  grand final.
- **List of competitions.** Creation order rather than latest first; no rename or delete; a full
  "Export" button on every row is heavy.
- **Good as it is:** keyboard flow through results (focus to the next open match, then the next
  round), accent- and case-insensitive search, option descriptions beside the list, the
  "(recommended)" tag, clear announcements (also of undo/redo), consistent dark theme, complete German
  (but E6), sound headings and landmarks, visible focus rings.

## Plan

Ordered by value for effort. Each step is one commit with its tests (unit tests in the app or the
engine where the logic is, an e2e test in `tournament-gui-e2e` for what only the page shows), and
DESIGN.md updated where a mechanism changes.

### 1. Phone overflow (L1, L2, L3) — done

- L1: `.columns` has `grid-template-columns: minmax(0, 1fr)` as its base, so the standings scroll
  inside their card (the summary's `dl` fits once the card does).
- L2: the setup's two columns only from 64rem; below, the cards stack.
- L3: the bar shows only icons below 60rem (the breakpoint of the columns), which leaves German
  room for a long word of the name.
- Tests: `tournament-gui-e2e/tests/layout.rs`, with the `Page` helpers `resize`,
  `expect_no_sideways_scroll` and `expect_room_for_words`.
- B7 moved to step 4, which changes the widths of pairings anyway.

### 2. Byes count when their round is reached (E1) — done

Done as (a): `Bye::is_reached`, `Phase::reach_byes` in `competition.rs`, DESIGN.md *Byes*
rewritten (the old rule and the other options under *Alternatives*), the test
`byes_are_scored_when_their_ranking_reaches_them` in `tests/full_tournament.rs`. The benchmark is
unchanged within its noise.

The decision "a bye is final when scheduled" holds for formats that schedule round by round, and is
wrong for those that schedule every round at once. Options:

- **(a, recommended)** A bye is *due* once its ranking has no open match in an earlier round of the
  phase (the same rule as `playable`). The phase keeps scheduled-but-not-due byes; after scheduling
  and after every reported result it runs `update_after_bye` for the byes that became due, then
  re-ranks. Deterministic (a function of state), costs one look at the reporting ranking's pending
  byes on the hot path, and needs no change to the formats or the calculators. Undoing a result
  is a replay, so a bye that stops being due needs no inverse.
- (b) Round robin, random rounds and Monster DYP schedule round by round. Loses the full schedule
  the operator sees up front, which the GUI's round select relies on.
- (c) Only hide it in the standings. Wrong: the metrics themselves are off, e.g. for a later phase.

**Decided: (a).** A bye is like a match reported automatically once it is playable in the sense of
"Playable now".

Steps for (a): rewrite DESIGN.md *Byes* ("final when scheduled" becomes "final when due"), add the
due check in `Competition` (`competition.rs` around the bye loop), adjust the doc of `Bye` and
`MetricsCalculator::update_after_bye`; tests: a round robin of 3 starts with everyone on 0 points,
the bye's point arrives after the ranking's earlier matches; the existing bye tests keep passing;
the benchmark doesn't regress.

### 3. The phase form's errors and submit (F1, F4, F8, P1) — done

- F1: submitting a form with issues (new phase or the dialog) moves the focus to the first list of
  issues, scrolled into view, instead of trying; rather than a disabled button, which would say
  nothing about why. `AddPhaseError::Invalid`/`EditPhaseError::Invalid` say "the form shows what
  to change" instead of the issues' raw English.
- F4: Enter in a number field takes the number without submitting.
- F8: `Session::start_refusal` (which `actions().start` uses) is shown next to Start.
- P1: the typed entry's error is under its field (`aria-invalid`, cleared when typing). The other
  errors' placement and the empty slot of the page's alert are left for step 9.
- F7 (warning on an empty ranking) moved to step 5: warnings that don't block are a new kind of
  issue in the form.

The original plan of this step:

- F1: in `tournament-app/src/error.rs`, recognise the incompatibility (downcast the
  `TransitionError` of `AddPhase`/`Replace` to the engine's incompatibility) and say it as a
  translated `Text` naming the plugins by label (`Arg::Plugin`), not the type path. In the GUI,
  disable "Add phase"/"Save changes" while `FormView` has issues (the issues already say why), with
  `aria-describedby` to the issues, so the refusal is only a fallback.
- F4: in `phase.rs`, stop Enter in a number input from submitting (`onkeydown` that commits the
  number on Enter without submitting), or move the submit button out of the `<form>`.
- F7: the form view gets an issue (a warning, not blocking) for an empty ranking; F8: `actions()`
  carries why Start is unavailable, shown next to it.
- P1: show an action's error next to what caused it: the typed entry's under its input (with
  `aria-invalid` and `aria-describedby`), the phase's at the submit button, the dialog's in the
  dialog; keep the page-wide alert for the rest. Reserve no empty slot when there is nothing to say.
- Tests: app unit test for the translated incompatibility; e2e: Add phase disabled with an issue,
  Enter in Rounds doesn't add the phase, the typed entry's error is in the entry.

### 4. Bracket geometry and affordance (B1, B2, B3, B4, B8) — done

- B1/B2: pairings fill their column; names wrap within 14rem; the slots of a column are a grid of
  `1fr` rows. Pairings without a match get an empty number cell, so that names line up.
- B3: open names in the accent, underlined, and a hint above the bracket.
- B4: a pairing decided by a bye recedes like a finished one; "Bye" is muted, not italic.
- B8: one scroller for every section, a shadow at the edge with more out of sight (on every
  `.scroll`, so the standings have it too), and the focus moving after a result scrolls only as
  far as it needs to (`focus_in_sight` in `play.rs`, the list of matches included).
- Tests: `expect_bracket_lines_meet` (measuring), in `layout.rs` for a single elimination with
  long names at laptop and phone size, and a double elimination of 6.
- B7 (16 entrants at 820px) moved to step 7: what widens the columns is the long placeholders
  ("Winner of Quarterfinals (1)"), which numbering every pairing shortens.

The original plan of this step:

- B1: `.slot > .pairing { flex: 1 }` (or `width: 100%`) so every pairing fills its column and the
  lines start at its edge.
- B2: let names wrap within a maximum pairing width (~14–16rem); make the first column's slots
  equal in height whatever their content (`display: grid; grid-auto-rows: 1fr` on its `ol`), since
  later columns derive their geometry from it (DESIGN.md *The bracket*).
- B3: give open names a visible button look on touch and keyboard (outline or underline always
  under `@media (hover: none)`, on `:focus-visible`), plus a one-line hint above the bracket
  ("Tap the winner's name").
- B4: bye pairings take the receded style of a finished one; "Bye" not in pending italics.
- B7: no needless scrollbar for 16 entrants at 820px (`--gap`, the pairings' widths).
- B8: one scroller for all sections of a double elimination; a fade at the right edge while more
  columns follow; focus moves with `scrollIntoView({ inline: "nearest", block: "nearest" })`.
- Tests: an e2e test with long names that compares each line's end with the middle of the pairing it
  leads to (the bracket tester's measuring script, as a `Gui` helper), at laptop and phone size.

### 5. The phase form follows the operator (F2, F3, F5, F6, F7, F9, L8) — done

- F2: the session keeps the config of the phase whose adding it undid; `Session::phase_form`
  starts out at it.
- F3: a list answered anew after a change leaves out what only a recommendation not chosen
  elsewhere needs, and keeps what it had that a plugin chosen elsewhere needs (generic, by the
  static declarations).
- F5: the GUI's number inputs are `type=text` with an `inputmode`; `PhaseForm::enter` reads `1e3`,
  `3.0` for whole numbers, and says "this takes …" for a number of another kind or size or nothing
  at all, "“abc” is no number" otherwise. Deviation: a comma is read as a decimal point in every
  language, as before (`1,000` in English is 1); a language-dependent reading would need the
  language in the form.
- F6: `ListInput::set`, recommended first, then by label; the GUI renders the set from it with the
  same elements whether checked or not.
- F7: `FormField::warnings`, shown in `.warnings`; a ranking of nothing is warned about.
- F9: a toggletip opens below its button unless the room there is short.
- L8: `phase::focus_later` after choosing, adding, moving and removing plugins and after adding
  the phase (to Start); undo, redo, previous/next round and ↑/↓ are `aria-disabled`.
- Found on the way: the e2e harness left one Chrome profile per test in `/tmp` (1,900 of them,
  13 GB, filling the tmpfs until IndexedDB failed); each test now has a profile directory of its
  own, removed at its end.

The original plan of this step:

- F2: when an undo removes the phase, rebuild the form from the undone phase's `Phase::config`
  (the session knows the action it walked through) instead of `Session::phase_form()`.
- F3: in `form.rs`, when later fields follow a change, keep (or add) the metrics that a kept
  criterion requires (`PluginEntry::providers_in`); don't tick metrics no criterion uses.
- F5: number inputs become `type=text inputmode=decimal` with the app's own parsing in
  `PhaseForm::enter`: accept `,` as decimal separator in German, refuse it with a message in English
  rather than reading `0,5` as 5, "this takes a whole number of at least 1" for `2.5`, the entered
  text in the message (never empty quotes), `1e3` handled.
- F6: keep the metric set in schema order in `FormView`, checked or not.
- F9, L8: keep or restore focus after list and select actions (focus the moved item's button, the
  next item after Remove); keep Previous/Next/Undo/Redo focusable when unavailable
  (`aria-disabled`), or move the focus to the select.
- Tests: app unit tests for F3, F5, F6; e2e for F2 and the focus.

### 6. A finished competition — done

- App: `Session::winners` (the teams ranked first once every match has a result, ties by name);
  `Progress::Won { winners }` for the list, the old `Finished` kept so stored listings still read.
- GUI: a banner above the views ("Winner: Dave", "Tied for first: Alice, Bob", with a trophy), the
  first three of the standings bold, the first tinted, and the phase folded into a `details` under
  its heading (still editable). The list says "Finished · winner: Dave".
- Tests: app unit tests (winners, ties, undo, an old listing reading); e2e for the banner, podium,
  undo, the list and a tie in German.

The original plan of this step:

- App: a `view` of the result once every match is played (winner, or those tied for first; top three
  for the standings), and the winner in `Listing`'s `Progress`.
- GUI: a banner above the columns ("Finished — winner: X"), the top three marked in the standings,
  "Edit phase" moved out of the way, the winner in the list of competitions.
- Tests: app unit test; e2e.

### 7. Bracket and reopen correctness of the view (E3, E4, E5, B5, B6, B7)

- E3: name the blocking match in the refusal (the engine knows which later result depends on it;
  the app maps it to a match number); in the app's match rows, mark a result that can't be reopened
  so the GUI disables Reopen with that reason.
- E4: in `session/bracket.rs`, drop pairings whose both sides are byes, the third-place pairing when
  a semifinal was a bye, and show the reset as "not needed" once the winner bracket's champion won
  the grand final. E5: "(if needed)" only while the reset isn't scheduled.
- B5: number every pairing from the start (positions in the bracket), so placeholders read "Winner of
  12"; English stage names "Round of 16"/"Round of 32"; one wording for sections and stages (the
  stage texts are in `plugins.ftl`, German too).
- B6: remember the view in local storage like the result entry.
- Tests: app unit tests of the bracket view; the engine's tag/locale tests for the new stage names.

### 8. Translation and text (E6, L5, L6, P2, P3, wording)

- E6: a metric column says its value as a `Text` the app translates (a key with the round as
  variable, like stages), and `Score`s are formatted by the language (`,` in German).
- L5: strip the isolation marks from the document title.
- L6: a translated import error, the parser's detail kept apart (e.g. in a `<details>`).
- P2: accept `1x`, `1-1`; name the match in "already has a result"; every malformed line the same
  message. P3: the search's empty message counts byes.
- Wording pass: one capitalisation for labels (the plugin label guidelines in
  `tournament_engine::plugins`, checked by its test), errors as sentences, "entrant" not "team",
  hide "Points for a draw" where the format has no draws (a fact the format declares).

### 9. Layout polish (look and feel, L4, L7, L9, L10, P4, E2)

- Sticky bar with Undo and the view switch; the storage warning collapsible to one line once seen.
- Match list: open matches first, or finished ones collapsed to one line each.
- Standings: short headers ("SB", "Bh") with the full name in the toggletip, consistent case; a
  "played" count.
- Buttons that look like buttons ("Remove", "Reopen", ↑/↓) with 44px touch targets; one primary
  action per card; a gap between "Edit phase" and "Start"; one ⓘ on the format row with the format's
  description under the select; range hints as helper text below the input, inputs on a grid.
- Entrant list grows with the page (no inner scroll box), the new entrant scrolled into view.
- L4: dialog left-aligned, sticky footer, darker backdrop in dark mode.
- L7: decide between the setup's column switch and stacking, and align DESIGN.md or the code.
- L9: latest first in the list of competitions, rename and delete, an imported copy named
  "Cup (imported)"; the full name of a cut header in a `title`. L10: warn (not refuse) on an entrant
  name equal to another's ignoring case and Unicode normalisation.
- P4: an undo that leaves play (undoing the start) asks first.
- E2: confirm the redraw; if the config feeds the rng, draw from the competition's seed only, or
  warn in the edit dialog that a change after the start redraws open rounds.
