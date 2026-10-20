# tournament-cli

An interactive command line interface for running a tournament with
[`tournament-engine`](../tournament-engine).

```text
cargo run -p tournament-cli -- [FILE]
```

Loads the competition saved to `FILE` if it exists, and otherwise starts a new
one that is saved there. Commands are then typed at a prompt until the session
is exited. The prompt shows the name of the file and marks unsaved changes with
a `*`.

## Example

```text
tournament> add Alice Bob "Carol C." Dave
4 entrant(s) added, 4 in total
tournament*> add-phase round-robin
phase added: round robin, ranked by more points, then more Sonneborn-Berger
tournament*> start
competition started, 6 match(es) scheduled
tournament*> matches
#1  round 1  Carol C.  vs  Dave      open
#2  round 1  Alice     vs  Bob       open
...
tournament*> report 1 a
#1: Carol C. beat Dave
tournament*> ranking
#  name      W-D-L  points  Sonneborn-Berger
1  Carol C.  1-0-0  1       0
2  Alice     0-0-0  0       0
...
tournament*> save cup.json
saved to cup.json
cup> exit
```

## Commands

| Command | Does |
|---|---|
| `entrants` | Lists the entrants. |
| `add-entrant <NAME>...` (`add`) | Adds entrants. Names must be unique; quote names containing spaces. If one name is invalid, none is added. |
| `remove-entrant <NAME>` (`remove`) | Removes an entrant. |
| `add-phase [FORMAT [FIELD=VALUE]...]` | Adds the phase the competition is played in: the format named, or, without a format, the phase configured at the prompt, see below. Only one phase is supported. |
| `start` | Starts the competition and schedules the first matches. Entrants and the phase cannot be changed afterwards. |
| `matches [--open] [--round <ROUND>]` | Lists the matches with their number, round, stage (for a format that names its stages, e.g. "Quarterfinals" in a knockout) and result, followed by the byes, i.e. who has no match in which round. `--open` leaves the byes out. |
| `report <NUMBER> <a\|b\|draw>` | Reports the result of a match: team a wins, team b wins, or a draw. |
| `undo-report <NUMBER>` | Undoes the result reported for a match, however long ago it was reported, keeping every other change. |
| `ranking` | Shows the current ranking with every metric the phase calculates. |
| `undo` / `redo` | Undoes the last change, or redoes the last undone one. |
| `save [PATH]` | Saves to `PATH`, or to the file loaded from or last saved to. |
| `exit [--force]` (`quit`) | Exits. Refuses while there are unsaved changes, unless `--force` is given. |
| `help [COMMAND]` | Shows the commands, or the help of one of them. |

Ctrl-C clears the line being typed, Ctrl-D behaves like `exit`.

## Adding a phase by its format

`add-phase <FORMAT>` adds a phase without asking anything: the format named,
with everything else — ranking, metrics, and every field of the format not
answered on the command line — as the format recommends. A format is named by
its label with hyphens for spaces, or by its id, so a format of another crate
that is linked in is named the same way. A field of the format is answered as
`FIELD=VALUE`, with `yes`/`no` for a yes-or-no field.

| Format | Fields (offered answer) | Plays | Ranked by |
|---|---|---|---|
| `round-robin` | — | Everyone plays everyone. | Points (1 for a win or a bye, 0.5 for a draw), then Sonneborn-Berger. |
| `single-elimination` | `third_place_match` (yes) | The loser of a match is out; draws are rejected. The two entrants knocked out in the semifinals play for third place, unless `third_place_match=no`, which leaves them third equal. | The round of the first loss, then the losses (only the round of the first loss without a match for third place). |
| `double-elimination` | `grand_final_reset` (yes) | An entrant is out after its second loss; draws are rejected. The champions of the winner and the loser bracket meet in the grand final, which is played once more if the champion of the winner bracket loses it, unless `grand_final_reset=no`. | The round of elimination, then the losses. |
| `random-rounds` | `rounds` (5) | That many randomly chosen rounds of a round robin. | Points (1 for a win or a bye, 0.5 for a draw), then Buchholz, Sonneborn-Berger and Buchholz of Buchholz. |
| `monster-dyp` | `rounds` (5) | That many rounds in each of which the entrants are drawn into random teams of two, each playing another such team. Entrants left over sit the round out, in turns. | Points (1 for a win or for sitting out, 0.5 for a draw). |

For example `add-phase random-rounds rounds=7` or
`add-phase single-elimination third_place_match=no`.

## Configuring a phase

`add-phase` without a format asks for everything about the phase instead, one
question at a time. The questions are not written for the CLI: they are the
schemas of the plugins the engine has registered, so a plugin of another crate
that is linked in is asked for the same way.

```text
tournament*> add-phase
? Format
  double elimination — an entrant is out after its second loss, draws are not allowed
  Monster DYP — every round, entrants are drawn into teams of two that play each other
  random rounds — randomly chosen rounds of a round robin
> round robin — everyone plays everyone
  single elimination — the loser of a match is out, draws are not allowed
? Ranked by
> more points
  more Sonneborn-Berger
  fewer losses
  fewer matches played — the same result out of fewer matches ranks higher
  …
  (no further one)
? Ranked by, then
> more Sonneborn-Berger
  …
? Ranked by, then
> (no further one)
? Metrics
  [x] win-draw-loss record — how many matches were won, drawn and lost
  [x] points — scored for every win, every draw and every bye
  [x] Sonneborn-Berger — the points of the opponents that were beaten or played to a draw
  [ ] Buchholz — the points of all opponents
  [ ] Buchholz of Buchholz — the Buchholz of all opponents
  [ ] match list — the matches an entrant played
  [ ] round of the first loss — the round in which an entrant lost for the first time
? Points: points for a win (1)
? Points: points for a draw (0.5)
? Points: points for a bye (1)
? Sonneborn-Berger: weight of a beaten opponent (1)
? Sonneborn-Berger: weight of an opponent played to a draw (0.5)
phase added: round robin, ranked by more points, then more Sonneborn-Berger
```

Every question after the format starts out at what the chosen format
recommends, so accepting all of them with Enter adds exactly what
`add-phase <FORMAT>` adds, and changing one thing means answering one question
differently. What the chosen format recommends is listed first, everything
else alphabetically. Enter confirms, Space toggles a metric, `↑`/`↓` move and typing
filters a list. Esc cancels the configuration and leaves the competition
unchanged, which is also what Ctrl-C does during it.

Only what fits the answers given so far is offered: a metric that needs a format
knocking entrants out is not offered for a round robin. The ranking is asked
for one criterion at a time, each one only breaking the ties of the ones before
it, until `(no further one)` is chosen. A metric the ranking needs starts out
chosen, and so does every metric a chosen one is derived from: ranking by
Sonneborn-Berger starts out calculating the points it sums and the results they
are scored from, and the ranking shows them. A metric with nothing to configure,
such as the results, is calculated even when it was not picked; one that has to
be configured, such as the points, is never added unasked. An answer a plugin rejects, such as a negative number of rounds, is asked
again right away, and a phase whose plugins do not fit together is asked for
again, starting out at the answers given.

Since this needs a terminal to ask at, a piped `add-phase` has to name a
format.

## Saving, undo and redo

A competition is saved as the JSON log of every change made to it, undos
included, not as a snapshot of its current state. Loading replays that log, so
undo and redo keep working across a save and a reload, and a file whose log no
longer replays is rejected instead of loading into a state no sequence of
changes could reach. A save is written to a temporary file next to the target
first, so a failed save never destroys the previous one.

`undo` reverts the latest change that is still in effect; repeated, it walks
further back. `redo` reapplies the latest undone change, but only until a new
change is made, as in an editor. Undo and redo are changes themselves, so they
count as unsaved.

`undo-report <NUMBER>` reaches a single result instead of the latest change: it
undoes the result reported for that match whenever it was reported, and every
result reported after it stays. A result another one depends on cannot be
undone on its own — undoing a semifinal would unschedule a final that already
has a result — so the dependent result has to be undone first. The match the
result was undone for is open again and can be reported anew, which is how a
result reported wrongly a while ago is corrected. Matches that were only
scheduled because of the undone result are unscheduled with it, which renumbers
the matches after them.

Every new competition gets a random seed, which makes the shuffled pairings
differ between competitions while keeping them reproducible from the log. The
seed is the first entry of the log and is never undone.

If the input is not a terminal, e.g. piped in, and it ends with unsaved
changes, the session ends with an error and those changes are discarded.
