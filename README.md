# Task-Manager für Linux – Rust

Ein nativer Task-Manager mit konfigurierbarer deutscher/englischer Oberfläche im Stil von Windows 10, vollständig
in Rust mit **egui/eframe** geschrieben. Die Live-Messwerte werden direkt aus
Linux `/proc` und `/sys` gelesen. Für CachyOS/Arch Linux unter x86_64 entwickelt.

## Seit 0.12.0 – Systemsprache und eigene Übersetzungen

Die Sprache der Linux-Sitzung wird beim Start erkannt: Deutsch für deutsche
Locales, Englisch für Englisch und alle noch nicht unterstützten Sprachen.
Die persönliche Datei `~/.config/linux-task-manager/languages.json` wird beim
normalen Start angelegt. Über `language` lässt sich die Auswahl fest einstellen;
unter `languages` können weitere Sprachen ohne Neubau hinzugefügt werden.
Fehlende Übersetzungen verwenden Englisch. Vorhandene Anpassungen bleiben bei
Updates erhalten. Seit 0.12.1 stehen englische Referenzschlüssel und der
englische Block zuerst in der Config; Deutsch ist eine Übersetzung. Details und Beispiel: [Sprachen konfigurieren](docs/Sprachen.md).

## Seit 0.8.0

**Optimieren → System scannen** zeigt Kandidaten und geschätztes CPU-/RAM-Potenzial.
Du kannst **Nur Analyse** verwenden oder **Auswählen & beenden** aktivieren,
einzelne Programme markieren und die Auswahl bestätigen. Kein Prozess wird
automatisch ausgewählt. System-/Hintergrunddienste, Container und erkannte
Anwendungshilfsprozesse sind geschützt. RAM ist eine RSS-Obergrenze, CPU eine
Momentaufnahme; tatsächliche Einsparung hängt von der Nutzung ab.

Der neue **Docker**-Reiter zeigt alle Container im aktuellen Docker-Kontext,
ihr Compose-Projekt, Status, CPU, RAM/Limit, kumulative Netzwerk-/Datenträger-E/A
und Prozesse/Threads. Docker-Abfragen laufen separat und nur bei sichtbarem
Reiter. Bestehende Docker CLI und Benutzerberechtigungen werden verwendet.

## Projekt-Vault

Unter `Vault/` liegt eine öffentliche Auswahl der Projektdokumentation mit
Projektübersicht, Architektur, Build-Anleitung, Entscheidungen und Aufgaben. In Obsidian **Ordner als Vault öffnen** wählen, den Ordner `Vault`
auswählen und mit der Notiz `Start` beginnen.

## Ohne Terminal starten

1. `release/Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz` herunterladen und im
   Dateimanager entpacken.
2. Die Datei **Task-Manager** doppelklicken. Falls der Dateimanager es verlangt:
   Eigenschaften → Berechtigungen → „Als Programm ausführbar“ aktivieren und
   „Ausführen“ wählen. Das TAR-Paket enthält bereits die Ausführungsrechte.
3. In der App **Datei → Für meinen Benutzer installieren** wählen.
4. Anschließend **Task-Manager** über das App-Menü öffnen.

Die Anwendung benötigt weder Python noch Qt oder eine Rust-Installation.
Sie benötigt eine grafische Linux-Sitzung (Wayland oder X11) sowie die üblichen
Linux-Systembibliotheken und einen OpenGL-/EGL-Treiber. Der fertige Build wird
auf CachyOS x86_64 getestet; andere Distributionen und ARM sind nicht verifiziert.

Die grafische Installation benötigt keine Administratorrechte und kopiert die
App nach `~/.local/share/linux-task-manager`. Sie legt einen App-Menü-Eintrag und
einen Terminalstarter unter `~/.local/bin/task-manager` an. `XDG_DATA_HOME` wird
berücksichtigt. Danach kann das entpackte Download-Paket gelöscht werden.
Eine vorhandene Python-Installation dieser App kann dadurch aktualisiert werden.
Deinstallation: **Datei → Deinstallieren**.

## Funktionen

- Prozessliste mit den aufklappbaren Gruppen **Anwendungen** und **Hintergrundprozesse**,
  Kategorieauswahl, Suche, numerischer Sortierung, PID und Benutzerfilter.
- CPU-Auslastung, Arbeitsspeicher (RSS) und E/A-Rate pro Prozess.
- GPU-Erkennung über PCI/DRM; verfügbare Auslastung, Grafikspeicher, Temperaturen,
  aktueller Grafik-Takt in MHz und Leistungsaufnahme in Watt.
- Prozessnamen unter Anwendungen/Hintergrundprozessen leicht eingerückt.
- Alle physischen Datenträger einzeln mit Modell, Kapazität sowie Lese-/Schreibrate.
- CPU-Gesamtauslastung und separate Diagramme für alle logischen Prozessoren gleichzeitig.
- Vollständig anklickbare Hardware-Karten und übersichtliche Messwert-/Temperaturkacheln.
- Aktueller CPU-Takt als Durchschnitt, physische Kerne und logische Prozessoren;
  Anzahl aller erfassten Prozesse und Software-Threads in der CPU-Leistungsansicht.
- Eigene RAM-Cache-Kachel (Cached + SReclaimable) mit Erklärung per Mauszeiger.
- Systemlaufzeit mit Tagen, Stunden, Minuten und Sekunden.
- Temperaturen für CPU, GPU, NVMe und RAM, soweit lesbare Sensoren vorhanden sind;
  weitere Sensoren in der Systemansicht.
- **Ansicht → Darkmode**: helle/dunkle Darstellung, beim nächsten Start beibehalten.
- Details mit Benutzer, Threads und kopierbarer Befehlszeile.
- Task beenden (SIGTERM) und Beenden erzwingen (SIGKILL), jeweils mit Rückfrage.
  Kernel-Prozesshandles (`pidfd`) und Startzeitprüfung verhindern das Signalisieren
  eines anderen Prozesses bei einer inzwischen wiederverwendeten PID.
- Neue Programme starten, Pause, Aktualisierungsintervall und Vordergrundmodus.
- Systeminformationen sowie grafische Benutzerinstallation und Deinstallation.

CPU-Werte der Prozesse sind auf die gesamte CPU-Kapazität normalisiert. Die erste
Messung eines Prozesses ist eine Baseline und liefert noch keine CPU-/E/A-Rate.
RAM pro Prozess ist RSS; gemeinsam genutzte Seiten können mehrfach vorkommen.
Die RAM-Gesamtanzeige verwendet `MemTotal - MemAvailable`.
Unlesbare E/A-Werte erscheinen als `—`. Datenträger werden ohne Partitionen und
ohne gestapelte virtuelle/RAM-Geräte einzeln angezeigt. Netzwerkschnittstellen werden
ohne Loopback summiert; VPNs und Bridges können Daten mehrfach zählen.
Benutzernamen kommen aus `/etc/passwd`; bei anderen Benutzerquellen wird die UID
angezeigt. Prozessrechte werden vom Kernel geprüft, die App erhöht keine Rechte.

GPU-Messwerte hängen vom Treiber ab: AMD über sysfs; NVIDIA zusätzlich über ein
vorhandenes `/usr/bin/nvidia-smi` (Abfrage mit Timeout). Intel und andere GPUs
werden erkannt, können aber ohne passende Treibermesswerte keine Auslastung oder
VRAM-Nutzung anzeigen. Fehlende Werte erscheinen als „nicht verfügbar“.
AMD-Takt stammt aus hwmon freq1_input (Hz → MHz), alternativ dem aktiven
pp_dpm_sclk-Eintrag. Leistung stammt aus power1_input, sonst power1_average
(µW → W). Der Tooltip erklärt Quelle/Mittelung; AMD-APUs können CPU-Leistung
einschließen. NVIDIA liest aktuellen Grafik-Takt und power.draw mit der
bestehenden NVIDIA-SMI-Abfrage. Keine Leistungsgrenzen oder Maximal-Takte
werden als aktuelle Messwerte ausgegeben. Andere Treiber bleiben ohne diese
implementierten Schnittstellen bei „nicht verfügbar“.
SATA-SMART-Abfragen und Intel-GPU-Auslastung über DRM-fdinfo sind nicht implementiert.
CPU-Kern-Temperaturen werden nur bei passenden Intel-coretemp-Sensoren samt
Paket-/Kernzuordnung angezeigt; ein Paketwert wird nicht auf einzelne Kerne kopiert.

Anwendungen werden über Fenster-PIDs erkannt: auf KDE Wayland über das Plasma-
Fensterprotokoll, auf X11 über EWMH. Die Desktop-Datei deklariert dafür
`X-KDE-Wayland-Interfaces=org_kde_plasma_window_management`. Ohne lesbare Fensterliste
verwendet die App Desktop-Einträge und kennzeichnet die Zuordnung als geschätzt.
Gleiche Programme werden nach UID, ausführbarer Datei, Anzeigename und Kategorie
summiert und einzeln aufgeklappt. Das ist keine automatische Prozessfamilienhierarchie.
Details zeigt weiterhin alle Prozesse einzeln.

CPU-Takt verwendet vorzugsweise CPUFreq-Hardwarewerte, danach `/proc/cpuinfo`
und zuletzt den Treiber-Takt. Die Quelle und die Zahl lesbarer CPUs stehen im
Tooltip. Physische Kerne werden aus SMT-Geschwistergruppen bzw. Paket-/Kern-
Identitäten gezählt, ohne pauschal die logische Anzahl zu halbieren. Gezählt
werden Kerne mit mindestens einem aktiven logischen Prozessor. Prozesse und
Threads beziehen sich auf alle lesbaren Einträge, unabhängig vom UI-Filter.
Der RAM-Cache enthält `Cached` (auch tmpfs/shared memory) plus `SReclaimable`,
keine zusätzlichen Buffers oder SwapCached. Er überlappt teilweise mit
verfügbarem RAM und wird nicht zusätzlich zur Gesamtbelegung addiert.

Die Darstellung wird unter `${XDG_CONFIG_HOME:-~/.config}/linux-task-manager/theme`
gespeichert. `--dark` und `--light` überschreiben die Darstellung nur für diesen Start.

## Terminal (optional)

```bash
./Task-Manager
./Task-Manager --install
~/.local/bin/task-manager
```

Wenn `~/.local/bin` im PATH steht, genügt `task-manager`.
Weitere Optionen: `--help`, `--version`, `--uninstall`, `--check` und
`--performance`, `--view cpu|memory|gpu|disk|network|system|processes|applications|background|docker|optimize|installed`, `--dark` und `--light`. Ohne Terminal stehen die normalen Funktionen in der App bereit.

## Entwickeln und bauen

Rust 1.88 oder neuer, C-Linker, pkg-config und die üblichen Linux-Desktop-Libraries:

Zum Erstellen des Download-Pakets werden zusätzlich Node.js, tar und sha256sum verwendet.

```bash
cargo run
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- --check
cargo run -- --smoke-test --screenshot artifacts/rust-prozesse.png
bash scripts/build-rust.sh
```

`--smoke-test` benötigt eine grafische Sitzung, prüft das echte Fenster mit
Live-Messungen und schließt es automatisch. Der Test beendet keine Prozesse und
nimmt keine Installation vor. Tests für Signale erzeugen eigene kurzlebige
Unterprozesse. Installationstests verwenden ausschließlich temporäre Ordner.

`scripts/build-rust.sh` baut mit `Cargo.lock` und erstellt das Download-Paket,
die Prüfsumme, den eigenen Quellcode und die Bibliothekslizenzen unter `release/`.
Für andere Distributionen in einer passenden Zielumgebung bauen und testen.
Der Code der Vorgängerversion liegt in `legacy/python/`.

Lizenz für eigenen Code: MIT. Bibliotheken: siehe `THIRD_PARTY.md`.

## Seit 0.8.0

Optimieren bietet einen Hintergrundfilter mit Typ-Spalte; Dienste/App-Helfer bleiben
geschützt. Gleiche Programme erscheinen mit Anzahl und Aufklapppfeil. Gruppenwerte
summieren sichtbare Mitglieder (RSS kann geteilte Seiten mehrfach enthalten).
Alle Leistungskurven haben sichtbare Farb-Legenden. Laufwerke zeigen Belegt,
Verfügbar und Dateisystemgröße eingebundener Dateisysteme mit Belegungsbalken.
Btrfs-Untervolumes/Bind-Mounts zählen einmal; unbekannte Partitionen bleiben unbekannt.

Docker: Zeile auswählen → Stoppen/Neu starten → Rückfrage bestätigen.
Die Aktion läuft separat und pinnt vollständige Container-ID und Docker-Ziel.
Docker kann nach zehn Sekunden zwangsweise beenden. Bei Verbindungsfehler vor
einem neuen Versuch aktualisieren, weil der Daemon den Auftrag schon erhalten
haben kann. Vorhandene Benutzerrechte; keine automatische Containeraktion.

Details stehen im Vault unter Prozessgruppen und Speicherbelegung sowie
Optimierung und Docker. Die Aktionstests verwenden simulierte Befehle;
vorhandene Nutzercontainer wurden nicht für Tests verändert.

## Installierte Apps (0.9.0)

Neuer Reiter im bisherigen Design: Apps-/Benutzer-/Systemkacheln, Suche,
Bereichsfilter, sortierbare Tabelle und App-Details. Pacman, Flatpak und
persönliche Desktop-Starter; optional weitere Pacman-Pakete ohne App-Eintrag.
Version, Paketbetreuer, Größe und Datum stammen aus vorhandenen Metadaten.
Unbekannte Daten bleiben unbekannt. Details neben der Liste oder bei kleinen
Fenstern über App-Details; Startbefehl kopierbar. Aktualisieren/F5 liest die Liste
neu, separat von den Live-Prozessmessungen. Deinstallation seit 0.11.0, siehe unten.
Optional: --view installed oder rein lesender --apps-check. Bedeutung und
Grenzen in Vault/01 Projekt/Installierte Apps.md.

## Treiber (0.10.0)

Neuer Reiter **Treiber** mit Kernelauswahl, Modulübersicht, Suche/Statusfiltern,
sortierbarer Tabelle und Details. **Nach Updates suchen** prüft installierte
Kernel-/Treiber-/Firmware-Pakete online über checkupdates (pacman-contrib).
Private temporäre Paketdatenbank, Abbruch und klares Ergebnis, keine Installation.
Fehlende Moduldateien des laufenden Kernels werden nach Paketupdates gekennzeichnet.
CLI: --view drivers, --drivers-check, --driver-updates-check. Datenbedeutung und
Grenzen stehen im Vault unter `01 Projekt/Treiber.md`.

## Paketaktionen (0.11.0)

Installierte Apps: App wählen → Deinstallieren → Vorschau → Deinstallation starten.
Pacman/Flatpak mit normaler Abhängigkeitsprüfung, persönliche Daten bleiben erhalten.
Manuelle Apps: Starter entfernen verschiebt nur die .desktop-Datei in den Papierkorb.
Treiber: Nach Updates suchen → Updates installieren … → Systemaktualisierung starten.
Das aktualisiert das gesamte CachyOS-System einschließlich sonstiger Paketupdates.
Grafische Administratorfreigabe; Ausgabe und einzelne Paketfragen im Task-Manager.
Keine Terminalpflicht, keine automatische Zustimmung. Nach Kernelupdates manuell
neu starten. Nachweise/Grenzen im Vault unter Version 0.11.0. 53 Tests bestanden;
echte Benutzerpakete wurden für Tests nicht entfernt oder aktualisiert.
