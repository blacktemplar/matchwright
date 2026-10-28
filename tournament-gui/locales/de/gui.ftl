# What the GUI says in German.

app-title = Matchwright
competition-title = { $name } – Matchwright
language = Sprache
theme = Farben
theme-system = Wie das System
theme-light = Hell
theme-dark = Dunkel
undo = Rückgängig
redo = Wiederherstellen
undo-start-title = Start rückgängig machen?
undo-start-description = Das Turnier kehrt zur Vorbereitung zurück, wo Teilnehmer und Phase geändert werden können. Wiederherstellen startet es wieder.
undo-start = Start rückgängig machen
not-persisted-summary = Dieser Browser behält die hier gespeicherten Turniere womöglich nicht.
not-persisted = Zum Beispiel in einem privaten Fenster, oder wenn ihm der Platz ausgeht. Exportiere sie, um eine Kopie zu behalten.
not-persisted-install = Als App installiert, darf die Seite sie meist behalten.
export-this-competition = Dieses Turnier exportieren
install = App installieren
dont-show-again = Nicht mehr anzeigen
error-detail = Einzelheiten
error-no-storage = der Browser lässt die Seite keine Turniere speichern
error-not-saved = die Änderungen konnten nicht gespeichert werden
error-not-exported = die Datei konnte nicht erstellt werden
source-code = Quellcode

## The help

help = Hilfe
help-title = Hilfe – Matchwright
help-stored = Deine Daten bleiben auf deinem Gerät
help-stored-here = Matchwright speichert alle Turniere lokal in deinem Browser (in dessen IndexedDB). Nichts, was du eingibst, verlässt dein Gerät: Es werden keine Daten an einen Server übertragen, es gibt kein Tracking, und du benötigst kein Konto.
help-stored-hosting = Die Seite selbst wird von GitHub Pages ausgeliefert, das wie jeder Webserver beim Laden deine IP-Adresse erhält.
help-stored-lost = Deine Turniere sind daher nur in diesem Browser auf diesem Gerät verfügbar. Sie werden gelöscht, wenn du die Websitedaten deines Browsers löschst oder ein privates Fenster schließt.
help-stored-export = Um eine Sicherungskopie zu erstellen oder auf einem anderen Gerät weiterzuarbeiten, exportiere das Turnier als Datei und importiere es dort.
help-offline = Offline-Nutzung und Installation
help-offline-text = Nach dem ersten Laden funktioniert Matchwright auch ohne Internetverbindung. Über das Menü deines Browsers kannst du es als App installieren; installierte Apps dürfen ihre Daten in der Regel dauerhaft speichern.
help-tabs = Ist ein Turnier in mehreren Tabs geöffnet, kann es jeweils nur in einem davon bearbeitet werden. Die anderen Tabs zeigen es schreibgeschützt an und werden automatisch aktualisiert.
help-open-source = Open Source
help-media-owner = Medieninhaber (Offenlegung gemäß § 25 MedienG): Benedikt Posch, Dornbirn, Österreich.
help-open-source-text =Matchwright ist freie Open-Source-Software unter der MIT- oder Apache-2.0-Lizenz. Der Quellcode ist auf GitHub verfügbar, wo du auch Fehler melden und Verbesserungen vorschlagen kannst.

## The competitions stored

competitions = Turniere
no-competitions = In diesem Browser ist noch kein Turnier gespeichert.
progress-setup = Nicht gestartet · { $entrants ->
        [one] ein Teilnehmer
       *[other] { $entrants } Teilnehmer
    }
progress-playing = Runde { $round } · { $open ->
        [one] ein Spiel offen
       *[other] { $open } Spiele offen
    }
progress-finished = Alle Spiele gespielt
progress-won = Beendet · { $winners }
export = Exportieren
export-competition = { $name } exportieren
rename = Umbenennen
rename-competition = { $name } umbenennen
rename-title = { $name } umbenennen
delete = Löschen
delete-competition = { $name } löschen
delete-title = { $name } löschen?
delete-description = Alles, was davon in diesem Browser gespeichert ist, wird entfernt. Exportiere es vorher, um eine Kopie zu behalten.
error-competition-open-elsewhere = das Turnier ist in einem anderen Tab offen; schließe es zuerst dort
error-not-deleted = das Turnier konnte nicht gelöscht werden
new-competition = Neues Turnier
create = Erstellen
import = Aus einer Datei importieren
error-empty-competition-name = ein Turnier braucht einen Namen
error-file-not-read = die Datei konnte nicht gelesen werden
error-competitions-unreadable = die gespeicherten Turniere können nicht gelesen werden

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
entrant-like = „{ $name }“ ist schon eingetragen.
remove = Entfernen
remove-entrant = { $name } entfernen
phase-heading = Phase
finished = Beendet
medal-gold = Goldmedaille,
medal-silver = Silbermedaille,
medal-bronze = Bronzemedaille,
add-phase = Phase hinzufügen
start = Starten

## Configuring the phase

choose-plugin = Auswählen …
no-plugin = (keines)
recommended = (empfohlen)
add-plugin = Hinzufügen
about = Über { $subject }
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
matches-and-standings = Spiele & Tabelle
rounds = Runden
round = Runde { $round }
round-with-open = Runde { $round } ({ $open } offen)
playable-now = Jetzt spielbar
playable-now-count = Jetzt spielbar ({ $count })
by-round = Nach Runde
previous-round = Vorige Runde
next-round = Nächste Runde
nothing-playable = Gerade kann kein Spiel gespielt werden.
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
played = Spiele
cross-table = Kreuztabelle
outcome-win = gewonnen
outcome-draw = unentschieden
outcome-loss = verloren
bracket = Turnierbaum
bracket-winners = Gewinnerseite
bracket-losers = Verliererseite
bracket-grand-final = Großes Finale
bracket-report-hint = Ein Klick oder Tippen auf den Namen der Seite, die gewonnen hat, trägt das Ergebnis ein.
bracket-bye = Freilos
winner-of-match = Sieger von { $number }
loser-of-match = Verlierer von { $number }
winner-of-stage = Sieger: { $stage }
loser-of-stage = Verlierer: { $stage }
stage-position = { $stage } ({ $position })
if-needed = falls nötig
bracket-not-needed = nicht nötig
type-results = Ergebnisse eintippen
enter-result = Ergebnis eingeben
enter-result-hint = die Nummer des Spiels, dann 1, wenn die linke Seite gewonnen hat{ $draws ->
        [yes] , X für ein Unentschieden
       *[no] {""}
    } oder 2, wenn die rechte Seite gewonnen hat, z. B. „3 1“; oder einen Teil des Namens des Siegers, mit der Nummer des Spiels, wo er zu mehreren passt{ $draws ->
        [yes] {" "}und X für ein Unentschieden, z. B. „ali“, „ali 5“ oder „ali X“
       *[no] , z. B. „ali“ oder „ali 5“
    }
result-entry-preview = Meldet Spiel { $number }: { $result }.
entry-found = Spiele zu „{ $text }“
entry-nothing-found = Kein Spiel passt zu „{ $text }“.
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
list-separator = {", "}
winners = { $count ->
        [one] Sieg: { $winners }
       *[other] geteilter erster Platz: { $winners }
    }
