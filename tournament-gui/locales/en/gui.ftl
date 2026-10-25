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
add-phase = Add phase
start = Start

## Configuring the phase

# what a choice of a plugin shows while none is chosen
choose-plugin = Choose…
# the choice of no plugin, and what the phase shows for a field without one
no-plugin = (none)
# next to a plugin the plugins chosen elsewhere recommend
recommended = (recommended)
# $field: the label of a list of plugins
add-to = Add to { $field }
# $plugin: a plugin in a list
move-up = Move { $plugin } up
move-down = Move { $plugin } down
remove-plugin = Remove { $plugin }
# $plugin: a plugin added to a list since another one needs it
implied-plugin = { $plugin }, since another one needs it
yes = yes
no = no
edit-phase = Edit phase
edit-phase-title = Edit the phase
edit-phase-description = The phase is played as if it had been added this way. The results reported stay as long as it still schedules their matches.
save-changes = Save changes
cancel = Cancel

## After the start

matches = Matches
# the lists of matches to choose from: those playable now, and each round
rounds = Rounds
round = Round { $round }
round-with-open = Round { $round } ({ $open } open)
# the matches whose entrants have played every round before
playable-now = Playable now
playable-now-count = Playable now ({ $count })
nothing-playable = No match can be played now.
match-number = #
# the header of the rounds of the matches
round-column = Round
# the header of the stages the format names, e.g. Semifinals
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
bye-in-stage = Bye: { $team } ({ $stage })
standings = Standings
rank = Rank
# reporting a result by typing it
enter-result = Enter a result
# $draws: yes if a match may end in a draw, else no
enter-result-hint = the number of the match, then 1 if side A won{ $draws ->
        [yes] , X for a draw,
       *[no] {""}
    } or 2 if side B won, e.g. “3 1”
report = Report

## What was done, as the page announces it

# $result: as `won` or `draw` say it
announce-reported = Match { $number }: { $result }.
announce-new-matches = { $count ->
        [one] One more match is scheduled.
       *[other] { $count } more matches are scheduled.
    }
announce-all-finished = Every match has a result.
announce-reopened = Match { $number } is open again.
announce-unscheduled = { $count ->
        [one] The match scheduled because of its result is gone.
       *[other] The { $count } matches scheduled because of its result are gone.
    }
# $action: one of the action-… messages
announce-undone = Undone: { $action }.
announce-redone = Redone: { $action }.
action-add-entrant = adding { $name }
action-remove-entrant = removing { $name }
action-add-phase = adding the phase
action-edit-phase = editing the phase
action-start = the start
# $result: as `won` or `draw` say it
action-report = the result of match { $number } ({ $result })
action-reopen = reopening match { $number }

## Names

# an entrant without a name, e.g. of a competition entered elsewhere
no-name = (no name)
# between the names of the members of a team
team-separator = {" & "}
