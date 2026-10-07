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
phase added: round robin, ranked by points, then Sonneborn-Berger
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
1  Carol C.  1-0-0  3       0
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
| `add-phase [PRESET]` | Adds the phase the competition is played in: the preset named, or, without a preset, the phase configured at the prompt, see below. Only one phase is supported. |
| `start` | Starts the competition and schedules the first matches. Entrants and the phase cannot be changed afterwards. |
| `matches [--open] [--round <ROUND>]` | Lists the matches with their number, round, stage (for a format that names its stages, e.g. "Quarterfinals" in a knockout) and result. |
| `report <NUMBER> <a\|b\|draw>` | Reports the result of a match: team a wins, team b wins, or a draw. |
| `undo-report <NUMBER>` | Undoes the result reported for a match, however long ago it was reported, keeping every other change. |
| `ranking` | Shows the current ranking with every metric the phase calculates. |
| `undo` / `redo` | Undoes the last change, or redoes the last undone one. |
| `save [PATH]` | Saves to `PATH`, or to the file loaded from or last saved to. |
| `exit [--force]` (`quit`) | Exits. Refuses while there are unsaved changes, unless `--force` is given. |
| `help [COMMAND]` | Shows the commands, or the help of one of them. |

Ctrl-C clears the line being typed, Ctrl-D behaves like `exit`.

## Phase presets

| Preset | Format | Ranked by |
|---|---|---|
| `round-robin` | Everyone plays everyone. | Points (3 for a win, 1 for a draw), then Sonneborn-Berger. |
| `single-elimination` | The loser of a match is out; draws are rejected. | The round of the first loss. |
| `double-elimination [--no-grand-final-reset]` | An entrant is out after its second loss; draws are rejected. The champions of the winner and the loser bracket meet in the grand final, which is played once more if the champion of the winner bracket loses it, unless `--no-grand-final-reset` is given. | The round of elimination, then the losses. |
| `random-rounds <ROUNDS>` | `ROUNDS` randomly chosen rounds of a round robin. | Points (3 for a win, 1 for a draw), then Buchholz, Sonneborn-Berger and Buchholz of Buchholz. |

## Configuring a phase

`add-phase` without a preset asks for everything about the phase instead, one
question at a time:

```text
tournament*> add-phase
? Phase format
> round robin — everyone plays everyone
  single elimination — the loser of a match is out, draws are not allowed
  double elimination — an entrant is out after its second loss, draws are not allowed
  random rounds — randomly chosen rounds of a round robin
? Metrics to calculate
> [x] W-D-L — how many matches were won, drawn and lost
  [x] points — scored for every win and every draw
  [ ] round of the first loss — the later the better, never having lost is best
  [ ] round of elimination — the later the better, still being in is best
  [ ] Buchholz — the points of all opponents
  [x] Sonneborn-Berger — the points of the opponents that were beaten or drawn
  [ ] Buchholz of Buchholz — the Buchholz of all opponents
? Points for a win (3)
? Points for a draw (1)
? Sonneborn-Berger: how much a beaten opponent counts (1)
? Sonneborn-Berger: how much an opponent played to a draw counts (0.5)
? Rank by
> points
  Sonneborn-Berger
  (no further metric)
? Then by
> Sonneborn-Berger
  (no further metric)
phase added: round robin, ranked by points, then Sonneborn-Berger
```

Every question starts out at the answer the chosen format comes with, so
accepting all of them with Enter adds exactly what the preset of that format
does, and changing one thing means answering one question differently. Enter
confirms, Space toggles a metric, `↑`/`↓` move and typing filters a list. Esc
cancels the configuration and leaves the competition unchanged, which is also
what Ctrl-C does during it.

The ranking is asked for one metric at a time, each one only breaking the ties
of the ones before it, until `(no further metric)` is chosen or no metric is
left. Only a metric that is calculated can be ranked by, and every metric a
calculated one is derived from is calculated as well even when it was not
picked: ranking by Sonneborn-Berger alone still calculates the points it sums
and the results they are scored from, and the ranking shows them.

Since this needs a terminal to ask at, a piped `add-phase` has to name a
preset.

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
