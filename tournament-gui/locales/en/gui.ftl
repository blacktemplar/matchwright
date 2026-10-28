# What the GUI says in English. Why an action was refused, and the plugins, are translated by
# tournament-app.

app-title = Tournament
# the title of the page while a competition is open
competition-title = { $name } – Tournament
language = Language
# whether the page is light or dark
theme = Colours
theme-system = Like the system
theme-light = Light
theme-dark = Dark
undo = Undo
redo = Redo
# undoing the start leaves play, so it is asked about first
undo-start-title = Undo the start?
undo-start-description = The competition goes back to its setup, where the entrants and the phase can be changed. Redo starts it again.
undo-start = Undo the start
# the browser refused to keep what is stored, which a private window does as well; the first line,
# which stays shown when the warning is folded, then what it unfolds to
not-persisted-summary = This browser may not keep the competitions stored here.
not-persisted = For example in a private window, or when it runs short of space. Export them to keep a copy.
# shown while the browser offers to install the page
not-persisted-install = Installed as an app, the page is usually allowed to keep them.
export-this-competition = Export this competition
install = Install the app
# hides the warning for good
dont-show-again = Don’t show again
# the errors below with a reason from the browser or the storage show it, in English, folded away
# below this
error-detail = Details
error-no-storage = the browser does not let the page store competitions
error-not-saved = the changes could not be saved
error-not-exported = the file could not be made

## The competitions stored

competitions = Competitions
no-competitions = No competition is stored in this browser yet.
# how far a competition of the list is
progress-setup = Not started · { $entrants ->
        [one] one entrant
       *[other] { $entrants } entrants
    }
# $round: the first round with a match to play; $open: its matches to play
progress-playing = Round { $round } · { $open ->
        [one] one match to play
       *[other] { $open } matches to play
    }
progress-finished = Every match played
# $winners: what the message winners says
progress-won = Finished · { $winners }
export = Export
export-competition = Export { $name }
rename = Rename
rename-competition = Rename { $name }
rename-title = Rename { $name }
delete = Delete
delete-competition = Delete { $name }
delete-title = Delete { $name }?
delete-description = Everything stored of it in this browser is removed. Export it first to keep a copy.
error-competition-open-elsewhere = the competition is open in another tab; close it there first
error-not-deleted = the competition could not be deleted
new-competition = New competition
create = Create
import = Import from a file
error-empty-competition-name = a competition needs a name
error-file-not-read = the file could not be read
error-competitions-unreadable = the competitions stored cannot be read

## The competition open

opening = Opening the competition…
all-competitions = All competitions
# what can be done with the competition as a whole
competition-actions = Competition
read-only = This competition is open in another tab, so it is only shown here. It follows what is done there, and can be changed here once that tab leaves it.

## Before the start

entrants = Entrants
no-entrants = No entrants yet. Each one is added by its name.
name = Name
add-entrant = Add entrant
# the name typed looks like that of an entrant, e.g. differing in case only; it is taken all the same
entrant-like = “{ $name }” is entered already.
remove = Remove
remove-entrant = Remove { $name }
# not `phase`, which is the app's message of the fields of a phase
phase-heading = Phase
# the heading once every match is played
finished = Finished
add-phase = Add phase
start = Start

## Configuring the phase

# what a choice of a plugin shows while none is chosen
choose-plugin = Choose…
# the choice of no plugin, and what the phase shows for a field without one
no-plugin = (none)
# next to a plugin the plugins chosen elsewhere recommend
recommended = (recommended)
# next to the choice of a plugin added to a list
add-plugin = Add
# the button showing what is said about $subject, a plugin or a field
about = About { $subject }
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
# what a narrow screen shows: the matches or the standings (or a knockout's bracket); a wide screen
# shows both, or the bracket
view = View
matches-and-standings = Matches & standings
# the lists of matches to choose from: those playable now, and each round
rounds = Rounds
round = Round { $round }
round-with-open = Round { $round } ({ $open } open)
# the matches whose entrants have played every round before
playable-now = Playable now
playable-now-count = Playable now ({ $count })
# shows the matches of one round at a time
by-round = By round
previous-round = Previous round
next-round = Next round
nothing-playable = No match can be played now.
# a field showing only the matches of entrants whose name contains what is typed
find-entrant = Find an entrant
nothing-found = No entrant here has a name containing “{ $text }”.
# said by screen readers before the number of a match, and between its sides
match = Match
versus = against
# what a screen reader says for the button reporting that a side won
wins = { $team } wins
# what that button shows, below the side's name; it must be a part of `wins`
wins-short = Wins
draw = Draw
won = { $team } won
reopen = Reopen
reopen-match = Reopen match { $number }
bye = Bye: { $team }
bye-in-stage = Bye: { $team } ({ $stage })
standings = Standings
rank = Rank
# the header of how many matches an entrant has a result of
played = Played
# the bracket of a knockout, every match in it from the start
bracket = Bracket
bracket-winners = Winners bracket
bracket-losers = Losers bracket
bracket-grand-final = Grand final
# above the bracket, while a match in it has no result: the names of its sides are buttons
bracket-report-hint = Click or tap the name of the side that won.
# a side of a match in the bracket that nobody takes
bracket-bye = Bye
# a side of a match in the bracket not decided yet, by the match or else the stage it comes from
winner-of-match = Winner of { $number }
loser-of-match = Loser of { $number }
winner-of-stage = Winner of { $stage }
loser-of-stage = Loser of { $stage }
# a match of a stage of several, by its place from the top of the stage's column
stage-position = { $stage } ({ $position })
# a match of the bracket only played if the one before ends a certain way, e.g. a grand final reset
if-needed = if needed
# a pairing only played if needed, once it is not
bracket-not-needed = not needed
# reporting a result by typing it
# what opens the field reporting results typed in
type-results = Type in results
enter-result = Enter a result
# $draws: yes if a match may end in a draw, else no
enter-result-hint = the number of the match, then 1 if the side on the left won{ $draws ->
        [yes] , X for a draw,
       *[no] {""}
    } or 2 if the side on the right won, e.g. “3 1”
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
# between the teams of a list, e.g. those tied for first
list-separator = {", "}
# $count: how many teams are ranked first; $winners: their names
winners = { $count ->
        [one] winner: { $winners }
       *[other] tied for first: { $winners }
    }
