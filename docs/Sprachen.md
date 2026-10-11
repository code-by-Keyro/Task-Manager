# Sprachen konfigurieren – Version 0.12.1

Die App fragt beim Start die Sprache der Linux-Sitzung ab. Deutsch (`de`, auch
`de_DE`, `de_AT` oder `de_CH`) verwendet deutsche Texte. Englisch sowie alle
noch nicht hinzugefügten Sprachen verwenden Englisch. Eine neue Sprache wird
sofort nach dem nächsten App-Start erkannt, wenn ihr Sprachcode in der
Konfiguration vorhanden ist. Das helle Windows-10-Layout bleibt erhalten.

## Persönliche Konfigurationsdatei

Beim ersten normalen Start wird die vollständige Datei hier angelegt:

`~/.config/linux-task-manager/languages.json`

Bei einem gültigen absoluten `XDG_CONFIG_HOME` liegt sie stattdessen unter
`$XDG_CONFIG_HOME/linux-task-manager/languages.json`. Die Datei enthält
`language`, die Sprachcodes unter `languages`, deren `decimal_separator` und
alle Übersetzungen unter `messages`. Bearbeite sie mit einem Texteditor und
starte die App neu. Es ist kein erneutes Bauen oder Installieren nötig.

- `"language": "auto"`: Sprache der Sitzung verwenden (Standard).
- `"language": "de"` oder `"en"`: Sprache fest einstellen.
- Eigener Sprachcode, z. B. `"fr"`: hinzugefügte Sprache fest einstellen.
- Ein unbekannter fest eingestellter Code verwendet Englisch und zeigt einen Hinweis.

Die Datei wird bei Updates **nicht überschrieben**. Eingebaute neue Texte und
fehlende Einträge werden ergänzt, ohne deine Datei zu verändern. Fehlt die Datei
oder ist sie nicht lesbar/kein gültiges JSON, startet die App mit den eingebauten
Sprachen. Eine fehlerhafte vorhandene Datei bleibt zum Korrigieren erhalten.

## Eine Sprache hinzufügen

Ergänze unter `languages` einen neuen Eintrag. Hier ein vollständiges kleines
Beispiel; nicht genannte Texte erscheinen weiterhin auf Englisch:

```json
{
  "language": "auto",
  "languages": {
    "fr": {
      "decimal_separator": ",",
      "messages": {
        "File": "Fichier",
        "View": "Affichage",
        "Help": "Aide",
        "Processes": "Processus",
        "Applications ({0})": "Applications ({0})"
      }
    }
  }
}
```

Deutsch und Englisch bleiben auch bei dieser kleinen Datei eingebaut. Für eine
vollständige Übersetzung kopiere den `messages`-Block aus `en` und
übersetze die **Werte rechts**, während die englischen Schlüssel links unverändert
bleiben. Die Referenz für Entwickler liegt in `config/languages.json` im Projekt
und im beigefügten Quellarchiv. Sprachcodes in `languages` sind kleingeschrieben,
z. B. `fr`, `es`, `pt_br`. Regionsvarianten fallen auf ihren Basiscode zurück.

Der englische Block steht als Referenz zuerst. Zum Beispiel ist `"File": "File"`
auf Englisch und `"File": "Datei"` auf Deutsch. `Running (container)` ist ein
Kontextschlüssel: Er wird als „Running“ angezeigt; der Prozessstatus verwendet
den separaten Schlüssel `Running`. `Kernel module` wird auf Englisch als
„Module“ angezeigt und unterscheidet sich vom Plural `Modules`. Alte deutsche Schlüssel aus 0.12.0 bleiben
kompatibel. Enthält eine Datei beide Schreibweisen, hat der englische Schlüssel
Vorrang. Die App verändert vorhandene Dateien nicht selbst.
Eine englische Anleitung liegt in [Languages.md](Languages.md).

`{0}`, `{1}` usw. stehen für Messwerte, Namen oder andere dynamische Angaben.
Sie müssen in der Übersetzung erhalten bleiben; Reihenfolge und Wiederholung
können geändert werden. Platzhalter werden geprüft. Ungültige/leere Texte
werden verworfen und durch den eingebauten Text bzw. Englisch ersetzt. `\n`
steht in JSON für einen Zeilenumbruch. JSON erlaubt keine Kommentare oder
überflüssigen Kommas. `decimal_separator` darf `.` oder `,` sein; neue Sprachen
verwenden ohne Angabe einen Punkt.

## Erkennung und Diagnose

Die effektive Nachrichtensprache folgt `LC_ALL`, dann `LC_MESSAGES`, dann `LANG`;
leere Werte werden übersprungen. Außerhalb der Locale `C`/`POSIX` wird die
Sprachprioritätenliste `LANGUAGE` bevorzugt (z. B. `fr:de:en`). Die erste
konfigurierte Sprache darin wird verwendet; ist keine bekannt, gilt Englisch.
`C`, `C.UTF-8`, `POSIX` und eine fehlende Locale verwenden Englisch.

Das folgt den [GNU-Regeln für Nachrichtensprachen](https://www.gnu.org/software/gettext/manual/html_node/The-LANGUAGE-variable.html).
Bei widersprüchlichen Umgebungsvariablen zählt die Sprache dieser gestarteten
Sitzung, nicht eine eventuell abweichende systemweite Voreinstellung.

`task-manager --language-check` zeigt Sprache, Pfad und Dezimaltrennzeichen.
Dieser Check und alle anderen Diagnose-/Smoke-Optionen legen keine Datei an.
Für Entwicklung und Tests kann `TASK_MANAGER_LANGUAGE_CONFIG` einen anderen
Dateipfad wählen. Damit lassen sich eigene Sprachen isoliert prüfen.

App-Menüs, Dialoge, eigene Meldungen, Statusanzeigen und Zahlen folgen der
gewählten Sprache. Namen/Beschreibungen von Desktop-Apps verwenden passende
vorhandene Desktop-Übersetzungen. Eigennamen, Dateipfade, technische Treiberdaten,
Paketbeschreibungen und externe Programmausgaben werden nicht automatisch
übersetzt. Maschinenlesbare Paket-/Docker-Abfragen behalten ihre feste
Ausgabesprache; die Sicherheit von Aktionen hängt nicht von Übersetzungen ab.
