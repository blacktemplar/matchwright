# The plugins of tournament-engine in German, see `PluginText` in `src/i18n.rs` for the keys.

## The fields of a phase itself, which are no plugin's

phase =
    .field-format = Format
    .field-format-description = wie die Spiele angesetzt werden
    .field-ranking = gewertet nach
    .field-ranking-description = die Besten zuerst; jedes Kriterium entscheidet nur, wo die vor ihm gleich sind
    .field-metrics = Kennzahlen
    .field-metrics-description = für jedes Team berechnet

## Phase formats

plugin-tournament_engine-round_robin =
    .label = Jeder gegen jeden
    .description = alle spielen gegen alle
plugin-tournament_engine-random_rounds =
    .label = zufällige Runden
    .description = zufällig gewählte Runden von Jeder gegen jeden
    .field-rounds = Runden
    .field-rounds-description = ist Jeder gegen jeden kürzer, wird das ganz gespielt
plugin-tournament_engine-single_elimination =
    .label = K.-o.-System
    .description = wer ein Spiel verliert, scheidet aus; Unentschieden sind nicht erlaubt
    .field-third_place_match = Spiel um Platz drei
    .field-third_place_match-description = die beiden Verlierer der Halbfinale spielen es neben dem Finale; ohne es sind beide gemeinsam Dritte
plugin-tournament_engine-double_elimination =
    .label = Doppel-K.-o.-System
    .description = wer zum zweiten Mal verliert, scheidet aus; Unentschieden sind nicht erlaubt
    .field-grand_final_reset = Wiederholung des großen Finales
    .field-grand_final_reset-description = verliert der Sieger der Gewinnerrunde das große Finale, spielen die beiden es noch einmal; ohne sie scheidet aus, wer das große Finale verliert
plugin-tournament_engine-monster_dyp =
    .label = Monster-DYP
    .description = jede Runde werden die Teilnehmer zu Zweierteams gelost, die gegeneinander spielen
    .field-rounds = Runden

## Metrics calculators

plugin-tournament_engine-points =
    .label = Punkte
    .description = für jeden Sieg, jedes Unentschieden und jedes Freilos
    .column = Punkte
    .field-points_for_win = Punkte für einen Sieg
    .field-points_for_draw = Punkte für ein Unentschieden
    .field-points_for_bye = Punkte für ein Freilos
    .field-points_for_bye-description = eine Runde ohne Spiel, z. B. bei einer ungeraden Zahl an Teilnehmern
plugin-tournament_engine-wins_draws_losses =
    .label = Bilanz aus Siegen, Unentschieden und Niederlagen
    .description = wie viele Spiele gewonnen, unentschieden gespielt und verloren wurden
    .column = S-U-N
plugin-tournament_engine-buchholz =
    .label = Buchholz
    .description = die Punkte aller Gegner
    .column = Buchholz
plugin-tournament_engine-buchholz_of_buchholz =
    .label = Fein-Buchholz
    .description = die Buchholz-Wertung aller Gegner
    .column = Fein-Buchholz
plugin-tournament_engine-sonneborn_berger =
    .label = Sonneborn-Berger
    .description = die Punkte der besiegten Gegner und derer, gegen die unentschieden gespielt wurde
    .column = Sonneborn-Berger
    .field-weight_for_win = Gewicht eines besiegten Gegners
    .field-weight_for_win-description = die Punkte dieses Gegners werden damit multipliziert
    .field-weight_for_draw = Gewicht eines Gegners, gegen den unentschieden gespielt wurde
    .field-weight_for_draw-description = die Punkte dieses Gegners werden damit multipliziert
plugin-tournament_engine-round_of_first_loss =
    .label = Runde der ersten Niederlage
    .description = die Runde, in der ein Teilnehmer zum ersten Mal verloren hat
    .column = verloren in
plugin-tournament_engine-round_of_elimination =
    .label = Runde des Ausscheidens
    .description = die Runde, in der ein Teilnehmer ausgeschieden ist
    .column = raus in

## Ranking criteria

plugin-tournament_engine-more_points =
    .label = mehr Punkte
plugin-tournament_engine-more_wins =
    .label = mehr Siege
plugin-tournament_engine-more_draws =
    .label = mehr Unentschieden
plugin-tournament_engine-fewer_losses =
    .label = weniger Niederlagen
plugin-tournament_engine-fewer_matches_played =
    .label = weniger gespielte Spiele
    .description = dasselbe Ergebnis aus weniger Spielen ist besser
plugin-tournament_engine-more_buchholz =
    .label = mehr Buchholz
plugin-tournament_engine-more_buchholz_of_buchholz =
    .label = mehr Fein-Buchholz
plugin-tournament_engine-more_sonneborn_berger =
    .label = mehr Sonneborn-Berger
plugin-tournament_engine-later_first_loss =
    .label = spätere erste Niederlage
    .description = nach der Runde der ersten Niederlage; nie verloren zu haben ist am besten
plugin-tournament_engine-later_elimination =
    .label = späteres Ausscheiden
    .description = nach der Runde des Ausscheidens; noch dabei zu sein ist am besten
