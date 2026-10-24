# What the GUI says in English. Why an action was refused, and the plugins, are translated by
# tournament-app.

app-title = Tournament
# the title of the page while a competition is open
competition-title = { $name } – Tournament
language = Language
undo = Undo
redo = Redo
not-persisted = The browser may delete the competitions stored here when it runs short of space. Export them to keep a copy.
# $reason: in English, from the browser
error-no-storage = the browser does not let the page store competitions: { $reason }
# $reason: in English, from the storage
error-not-saved = the changes could not be saved: { $reason }
# $reason: from the browser
error-not-exported = the file could not be made: { $reason }

## The competitions stored

competitions = Competitions
no-competitions = No competition is stored in this browser yet.
export = Export
export-competition = Export { $name }
new-competition = New competition
create = Create
import = Import from a file
error-empty-competition-name = a competition needs a name
# $reason: in English, from the browser
error-file-not-read = the file could not be read: { $reason }
# $reason: in English, from the storage
error-competitions-unreadable = the competitions stored cannot be read: { $reason }

## The competition open

opening = Opening the competition…
all-competitions = All competitions
# what can be done with the competition as a whole
competition-actions = Competition
read-only = This competition is open in another tab, so it is only shown here. It follows what is done there, and can be changed here once that tab leaves it.

## Before the start

entrants = Entrants
name = Name
add-entrant = Add entrant
remove = Remove
remove-entrant = Remove { $name }
phase = Phase
format = Format
add-phase = Add phase
phase-added = The phase is added, played with the format's defaults.
start = Start

## After the start

matches = Matches
rounds = Rounds
round = Round { $round }
round-with-open = Round { $round } ({ $open } open)
match-number = #
# the header of the stages, which are in English, as the format names them
stage = Stage
side-a = Side A
side-b = Side B
result = Result
wins = { $team } wins
draw = Draw
won = { $team } won
reopen = Reopen
reopen-match = Reopen match { $number }
bye = Bye: { $team }
standings = Standings
rank = Rank

## Names

# an entrant without a name, e.g. of a competition entered elsewhere
no-name = (no name)
# between the names of the members of a team
team-separator = {" & "}
