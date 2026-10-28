# What tournament-app says in English: why an action was refused. Each message says what the
# Display of its error says.

error-empty-name = an entrant name must not be empty
error-duplicate-name = there already is an entrant named “{ $name }”
error-entrant-added-after-start = the competition has started, so no entrant can be added anymore
error-unknown-entrant = there is no such entrant
error-entrant-removed-after-start = the competition has started, so no entrant can be removed anymore
error-unknown-format = there is no format “{ $format }”
# $reason: in English, from the format's code
error-format-misconfigured = { $reason }
error-invalid-phase = the phase cannot be played this way; the form shows what to change
error-phase-already-added = the competition already has a phase, and only one is supported
error-phase-added-after-start = the competition has started, so no phase can be added anymore
error-no-phase = the competition has no phase yet
# $reason: in English, from the engine
error-phase-change-does-not-fit = the phase cannot be changed this way, since a later change no longer fits it: { $reason }
error-started-already = the competition has started already
error-too-few-entrants = a competition needs at least two entrants
error-not-started = the competition has not started yet
error-unknown-match = there is no such match
error-match-finished-already = the match has a result already
error-draw-not-allowed = a match of this format cannot end in a draw
error-result-unreadable = enter the number of a match and its result, 1, X or 2, e.g. “3 1”, or part of the winner's name
error-no-match-numbered = there is no match { $number }
error-match-numbered-finished-already = match { $number } has a result already
error-result-missing = enter the result of match { $number } as well: 1, X or 2
# $entered: part of the winner's name, as entered
error-no-name-found = no open match has an entrant whose name contains “{ $entered }”
error-name-not-in-match = no open match { $number } has an entrant whose name contains “{ $name }”
# $matches: how many open matches have an entrant whose name contains $name; $number: one of them
error-name-in-several-matches = “{ $name }” fits { $matches } open matches; add the number of one, e.g. “{ $name } { $number }”
error-name-on-both-sides = “{ $name }” fits both sides of match { $number }; enter more of the name
error-match-not-reported = the match has no result that could be undone
error-result-depended-on = a later result depends on this one; undo that one first
# $number: the number of the match whose result depends on the one to reopen
error-result-depended-on-by = the result of match { $number } depends on this one; reopen that match first
error-nothing-to-undo = there is nothing to undo
error-nothing-to-redo = there is nothing to redo
error-competition-not-found = there is no such competition
# the messages below have a detail, in English, saying why: from the storage, from reading JSON or
# from the engine
error-competition-unreadable = the competition cannot be read
error-log-entry-unreadable = entry { $position } of the log cannot be read
error-person-unreadable = an entrant’s name cannot be read
error-log-does-not-replay = the log does not replay
error-file-unreadable = the file is no competition this version can open
# $reason: in English, why the engine refused an action
error-rejected = { $reason }

## The form of a phase

# the numbers a field takes; $whole: yes or no, $bounds: both, min, max or none, $multiple: 0 for
# any number
number-range = { $whole ->
        [yes] a whole number
       *[no] a number
    }{ $bounds ->
        [both] {" "}from { $min } to { $max }
        [min] {" "}of at least { $min }
        [max] {" "}of at most { $max }
       *[none] {""}
    }{ $multiple ->
        [0] {""}
       *[other] {" "}that is a multiple of { $multiple }
    }
# what an answer leads to that the operator may not want, although the phase can be played so
warning-unranked = ranked by nothing, every entrant is tied in the standings
# what is wrong with the answer of a field, or with a plugin chosen
issue-unanswered = this needs an answer
# the variables of number-range
issue-out-of-range = this takes { number-range }
# $entered: what the operator typed
issue-not-a-number = “{ $entered }” is no number
issue-duplicate = this is chosen twice
# $reason: in English, from the plugin
issue-rejected = this is not taken: { $reason }
# $reason: in English
issue-invalid = { $reason }
# $reason: in English
issue-incompatible = this does not fit next to the others: { $reason }
# $plugin: a plugin, $fact: in English, what it needs
issue-unprovided = { $plugin } needs { $fact }, which nothing provides
# $plugin: a plugin, $providers: the plugins of which it needs one
issue-unprovided-by = { $plugin } needs one of these: { $providers }
