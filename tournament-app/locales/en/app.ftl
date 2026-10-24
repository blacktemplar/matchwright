# What tournament-app says in English: why an action was refused. Each message says what the
# Display of its error says.

error-empty-name = an entrant name must not be empty
error-duplicate-name = there already is an entrant named "{ $name }"
error-entrant-added-after-start = the competition has started, so no entrant can be added anymore
error-unknown-entrant = there is no such entrant
error-entrant-removed-after-start = the competition has started, so no entrant can be removed anymore
error-unknown-format = there is no format "{ $format }"
# $reason: in English, from the format's code
error-format-misconfigured = { $reason }
# $issues: in English
error-invalid-phase = the phase cannot be played this way: { $issues }
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
error-match-not-reported = the match has no result that could be undone
error-result-depended-on = a later result depends on this one; undo that one first
error-nothing-to-undo = there is nothing to undo
error-nothing-to-redo = there is nothing to redo
error-competition-not-found = there is no such competition
# $reason: in English, from the storage
error-competition-unreadable = the competition cannot be read: { $reason }
# $reason: in English, from reading JSON
error-log-entry-unreadable = entry { $position } of the log cannot be read: { $reason }
# $reason: in English, from reading JSON
error-person-unreadable = an entrant's name cannot be read: { $reason }
# $reason: in English, from the engine
error-log-does-not-replay = the log does not replay: { $reason }
# $reason: in English, from reading JSON or from the engine
error-file-unreadable = the file is no competition this version can open: { $reason }
# $reason: in English, why the engine refused an action
error-rejected = { $reason }
