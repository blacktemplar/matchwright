# What tournament-app says in German: why an action was refused. A variable described as English in
# `en/app.ftl` is English here too.

error-empty-name = der Name eines Teilnehmers darf nicht leer sein
error-duplicate-name = es gibt schon einen Teilnehmer namens „{ $name }“
error-entrant-added-after-start = das Turnier hat begonnen, daher kann kein Teilnehmer mehr hinzugefügt werden
error-unknown-entrant = diesen Teilnehmer gibt es nicht
error-entrant-removed-after-start = das Turnier hat begonnen, daher kann kein Teilnehmer mehr entfernt werden
error-unknown-format = es gibt kein Format „{ $format }“
error-format-misconfigured = das Format ist fehlerhaft: { $reason }
error-invalid-phase = so kann die Phase nicht gespielt werden; das Formular zeigt, was zu ändern ist
error-phase-already-added = das Turnier hat schon eine Phase, und mehr als eine wird nicht unterstützt
error-phase-added-after-start = das Turnier hat begonnen, daher kann keine Phase mehr hinzugefügt werden
error-no-phase = das Turnier hat noch keine Phase
error-phase-change-does-not-fit = so kann die Phase nicht geändert werden, da eine spätere Änderung nicht mehr dazu passt: { $reason }
error-started-already = das Turnier hat schon begonnen
error-too-few-entrants = ein Turnier braucht mindestens zwei Teilnehmer
error-not-started = das Turnier hat noch nicht begonnen
error-unknown-match = dieses Spiel gibt es nicht
error-match-finished-already = das Spiel hat schon ein Ergebnis
error-draw-not-allowed = ein Spiel dieses Formats kann nicht unentschieden enden
error-result-unreadable = gib die Nummer eines Spiels und sein Ergebnis ein, 1, X oder 2, z. B. „3 1“
error-no-match-numbered = es gibt kein Spiel { $number }
error-match-not-reported = das Spiel hat kein Ergebnis, das zurückgenommen werden könnte
error-result-depended-on = ein späteres Ergebnis hängt von diesem ab; nimm zuerst jenes zurück
error-nothing-to-undo = es gibt nichts rückgängig zu machen
error-nothing-to-redo = es gibt nichts wiederherzustellen
error-competition-not-found = dieses Turnier gibt es nicht
error-competition-unreadable = das Turnier kann nicht gelesen werden: { $reason }
error-log-entry-unreadable = Eintrag { $position } des Protokolls kann nicht gelesen werden: { $reason }
error-person-unreadable = der Name eines Teilnehmers kann nicht gelesen werden: { $reason }
error-log-does-not-replay = das Protokoll lässt sich nicht nachspielen: { $reason }
error-file-unreadable = die Datei ist kein Turnier, das diese Version öffnen kann: { $reason }
error-rejected = abgelehnt: { $reason }

## The form of a phase

number-range = { $whole ->
        [yes] eine ganze Zahl
       *[no] eine Zahl
    }{ $bounds ->
        [both] {" "}von { $min } bis { $max }
        [min] {" "}ab { $min }
        [max] {" "}bis { $max }
       *[none] {""}
    }{ $multiple ->
        [0] {""}
       *[other] , die ein Vielfaches von { $multiple } ist
    }
issue-unanswered = das braucht eine Antwort
issue-out-of-range = das muss { number-range } sein
issue-not-a-number = „{ $entered }“ ist keine Zahl
issue-duplicate = das ist doppelt gewählt
issue-rejected = das wird nicht angenommen: { $reason }
issue-invalid = { $reason }
issue-incompatible = das passt nicht zu den anderen: { $reason }
issue-unprovided = { $plugin } braucht { $fact }, was nichts liefert
issue-unprovided-by = { $plugin } braucht eines davon: { $providers }
