# Task-Manager für Linux

Ein nativer, deutschsprachiger Task-Manager mit einer Oberfläche im Stil von
Windows 10. Für CachyOS/Arch Linux unter x86_64 entwickelt, mit Qt 6 und psutil.

## Ohne Terminal starten

1. `Task-Manager-0.1.0-Linux-x86_64.tar.gz` aus `release/` herunterladen und im
   Dateimanager entpacken.
2. `Task-Manager` doppelklicken. Falls der Dateimanager es verlangt, unter
   Eigenschaften → Berechtigungen „Als Programm ausführbar“ aktivieren und
   „Ausführen“ wählen. Das TAR-Paket enthält bereits die Ausführungsrechte.
3. In der App **Datei → Für meinen Benutzer installieren** wählen.
4. Anschließend die App über **Task-Manager** im App-Menü öffnen.

Python und Qt sind im Paket enthalten. Die Installation benötigt keine
Administratorrechte. Sie kopiert die App nach `~/.local/share/linux-task-manager`,
legt eine `.desktop`-Datei im Benutzer-App-Menü und einen Terminalstarter unter
`~/.local/bin/task-manager` an. `XDG_DATA_HOME` wird berücksichtigt.
Das entpackte Paket kann nach der Installation gelöscht werden.
Deinstallation: **Datei → Deinstallieren**.

Die fertige Datei wurde auf CachyOS gebaut. Eine grafische Linux-Sitzung und die
üblichen Systembibliotheken für Wayland oder X11 müssen vorhanden sein. Andere
Distributionen sowie ARM werden mit diesem Paket nicht als unterstützt zugesagt.

## Funktionen

- Live-Prozessliste mit Suche, numerischer Sortierung und Benutzerfilter.
- CPU-Auslastung, Arbeitsspeicher, E/A-Rate und Prozess-ID.
- Leistungsdiagramme für CPU, RAM, Datenträger und Netzwerk; einzelne CPU-Kerne.
- Details mit Benutzer, Threads und kopierbarer Befehlszeile.
- Task beenden (SIGTERM) und Beenden erzwingen (SIGKILL), mit Rückfrage und
  Prüfung der Prozessidentität. Fremde Prozesse nur, soweit Linux es erlaubt.
- Neue Programme starten, Pause, Aktualisierungsintervall, immer im Vordergrund.
- Systeminformationen sowie grafische Benutzerinstallation.

CPU-Werte der Prozesse sind wie bei Windows auf die gesamte CPU-Kapazität
normalisiert. Der erste Messzyklus liefert noch keine aussagekräftige Prozess-
CPU-Rate. RAM in der Prozessliste ist RSS, gemeinsam genutzte Seiten können
mehrfach vorkommen; die RAM-Gesamtanzeige basiert auf `total - available`.
Unlesbare E/A-Werte erscheinen als `—`. Disk-Zähler und Netzwerkschnittstellen
werden summiert; virtuelle Geräte, VPNs und Bridges können mehrfach zählen.
Es werden keine erfundenen GPU-, Autostart- oder Netzwerkwerte pro Prozess angezeigt.

## Terminal (optional)

```bash
./Task-Manager
./Task-Manager --install
~/.local/bin/task-manager
```

Wenn `~/.local/bin` im PATH steht, genügt nach der Installation `task-manager`.
`--help`, `--version` und `--uninstall` sind ebenfalls verfügbar.

## Entwickeln und selbst bauen

Python 3.11 oder neuer:

```bash
python -m venv .venv
.venv/bin/python -m pip install -r requirements-lock.txt
.venv/bin/python -m task_manager
QT_QPA_PLATFORM=offscreen .venv/bin/python -m unittest discover -s tests
QT_QPA_PLATFORM=offscreen .venv/bin/python -m task_manager --smoke-test
.venv/bin/python scripts/build.py
```

`release/` enthält danach ein portables Download-Paket, den App-Quellcode und
SHA256-Prüfsumme. Der Build benötigt Linux x86_64. Für andere Distributionen
auf einer passenden bzw. älteren Zielumgebung bauen und dort testen.
Die App lässt sich alternativ als Python-Paket mit `pip install .` installieren.

Lizenz für eigenen Code: MIT. Bibliotheken: siehe `THIRD_PARTY.md`.
