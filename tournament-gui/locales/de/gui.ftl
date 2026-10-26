# What the GUI says in German.

app-title = Turnier
competition-title = { $name } – Turnier
language = Sprache
theme = Farben
theme-system = Wie das System
theme-light = Hell
theme-dark = Dunkel
undo = Rückgängig
redo = Wiederherstellen
not-persisted = Dieser Browser behält die hier gespeicherten Turniere womöglich nicht, z. B. in einem privaten Fenster oder wenn ihm der Platz ausgeht. Exportiere sie, um eine Kopie zu behalten.
not-persisted-install = Als App installiert, darf die Seite sie meist behalten.
export-this-competition = Dieses Turnier exportieren
install = App installieren
dont-show-again = Nicht mehr anzeigen
error-no-storage = der Browser lässt die Seite keine Turniere speichern: { $reason }
error-not-saved = die Änderungen konnten nicht gespeichert werden: { $reason }
error-not-exported = die Datei konnte nicht erstellt werden: { $reason }

## The competitions stored

competitions = Turniere
no-competitions = In diesem Browser ist noch kein Turnier gespeichert.
export = Exportieren
export-competition = { $name } exportieren
new-competition = Neues Turnier
create = Erstellen
import = Aus einer Datei importieren
error-empty-competition-name = ein Turnier braucht einen Namen
error-file-not-read = die Datei konnte nicht gelesen werden: { $reason }
error-competitions-unreadable = die gespeicherten Turniere können nicht gelesen werden: { $reason }

## The competition open

opening = Das Turnier wird geöffnet …
all-competitions = Alle Turniere
competition-actions = Turnier
read-only = Dieses Turnier ist in einem anderen Tab geöffnet und wird hier nur angezeigt. Es folgt dem, was dort getan wird, und kann hier geändert werden, sobald jener Tab es verlässt.

## Before the start

entrants = Teilnehmer
no-entrants = Noch keine Teilnehmer. Jeder wird mit seinem Namen hinzugefügt.
name = Name
add-entrant = Teilnehmer hinzufügen
remove = Entfernen
remove-entrant = { $name } entfernen
phase = Phase
add-phase = Phase hinzufügen
start = Starten

## Configuring the phase

choose-plugin = Auswählen …
no-plugin = (keines)
recommended = (empfohlen)
add-to = Hinzufügen zu { $field }
move-up = { $plugin } nach oben schieben
move-down = { $plugin } nach unten schieben
remove-plugin = { $plugin } entfernen
implied-plugin = { $plugin }, da ein anderes es braucht
yes = ja
no = nein
edit-phase = Phase bearbeiten
edit-phase-title = Die Phase bearbeiten
edit-phase-description = Die Phase wird gespielt, als wäre sie so hinzugefügt worden. Die gemeldeten Ergebnisse bleiben, solange sie deren Spiele noch ansetzt.
save-changes = Änderungen speichern
cancel = Abbrechen

## After the start

matches = Spiele
view = Ansicht
rounds = Runden
round = Runde { $round }
round-with-open = Runde { $round } ({ $open } offen)
playable-now = Jetzt spielbar
playable-now-count = Jetzt spielbar ({ $count })
by-round = Nach Runde
previous-round = Vorige Runde
next-round = Nächste Runde
nothing-playable = Gerade kann kein Spiel gespielt werden.
find-entrant = Teilnehmer suchen
nothing-found = Kein Spiel hier hat einen Teilnehmer, dessen Name „{ $text }“ enthält.
match = Spiel
versus = gegen
wins = { $team } gewinnt
wins-short = Gewinnt
draw = Unentschieden
won = { $team } hat gewonnen
reopen = Wieder öffnen
reopen-match = Spiel { $number } wieder öffnen
bye = Freilos: { $team }
bye-in-stage = Freilos: { $team } ({ $stage })
standings = Tabelle
rank = Platz
enter-result = Ergebnis eingeben
enter-result-hint = die Nummer des Spiels, dann 1, wenn die linke Seite gewonnen hat{ $draws ->
        [yes] , X für ein Unentschieden
       *[no] {""}
    } oder 2, wenn die rechte Seite gewonnen hat, z. B. „3 1“
report = Melden

## What was done, as the page announces it

announce-reported = Spiel { $number }: { $result }.
announce-new-matches = { $count ->
        [one] Ein weiteres Spiel ist angesetzt.
       *[other] { $count } weitere Spiele sind angesetzt.
    }
announce-all-finished = Jedes Spiel hat ein Ergebnis.
announce-reopened = Spiel { $number } ist wieder offen.
announce-unscheduled = { $count ->
        [one] Das wegen seines Ergebnisses angesetzte Spiel entfällt.
       *[other] Die { $count } wegen seines Ergebnisses angesetzten Spiele entfallen.
    }
announce-undone = Rückgängig gemacht: { $action }.
announce-redone = Wiederhergestellt: { $action }.
action-add-entrant = Hinzufügen von { $name }
action-remove-entrant = Entfernen von { $name }
action-add-phase = Hinzufügen der Phase
action-edit-phase = Bearbeiten der Phase
action-start = Start
action-report = Ergebnis von Spiel { $number } ({ $result })
action-reopen = Wiederöffnen von Spiel { $number }

## Names

no-name = (ohne Namen)
team-separator = {" & "}
