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
phase added
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
| `add-phase <PRESET>` | Adds the phase the competition is played in, see below. Only one phase is supported. |
| `start` | Starts the competition and schedules the first matches. Entrants and the phase cannot be changed afterwards. |
| `matches [--open]` | Lists the matches with their number, round and result. |
| `report <NUMBER> <a\|b\|draw>` | Reports the result of a match: team a wins, team b wins, or a draw. |
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
| `random-rounds <ROUNDS>` | `ROUNDS` randomly chosen rounds of a round robin. | Points (3 for a win, 1 for a draw), then Buchholz, Sonneborn-Berger and Buchholz of Buchholz. |

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

Every new competition gets a random seed, which makes the shuffled pairings
differ between competitions while keeping them reproducible from the log. The
seed is the first entry of the log and is never undone.

If the input is not a terminal, e.g. piped in, and it ends with unsaved
changes, the session ends with an error and those changes are discarded.
