<img src="assets/task-manager.svg" alt="Task-Manager" width="64">

# Task-Manager für Linux

Ein nativer Desktop-Task-Manager für Linux, geschrieben in **Rust** mit
**egui/eframe**. Prozesse, Hardware und Systemleistung in einer Oberfläche im
Stil von Windows 10 – mit Darkmode und konfigurierbaren Übersetzungen.

**Stand: 0.12.1 · Linux x86_64 · MIT-Lizenz**

## Funktionen

- **Prozesse:** Anwendungen und Hintergrundprozesse, Suche, Sortierung,
  Benutzerfilter, CPU-/RAM-/E/A-Werte und Prozessdetails.
- **Leistung:** CPU-Gesamtlast und einzelne logische Prozessoren, Arbeitsspeicher,
  GPU, physische Datenträger, Netzwerk, Temperaturen und Systemlaufzeit.
- **Docker:** Container und Compose-Projekte mit Ressourcenwerten; Stoppen und
  Neustarten mit Rückfrage über die vorhandene Docker CLI.
- **Optimierung:** Laufende Programme analysieren und geschätztes CPU-/RAM-Potenzial
  anzeigen; ausgewählte Prozesse erst nach Bestätigung beenden.
- **Apps und Treiber:** Installierte Anwendungen und Kernelmodule anzeigen;
  Paketaktionen für Pacman/Flatpak und bestätigte Systemaktualisierung.
- **Darstellung:** Helles Design, gespeicherter Darkmode, Deutsch und Englisch
  sowie eigene Übersetzungen über JSON.

Prozess-, Container- und Paketaktionen verwenden die bestehenden Benutzerrechte
und verlangen eine Bestätigung. Die App führt diese Aktionen nicht automatisch aus.

## Bauen und starten

Das Repository enthält die Quellen. Ein fertiges Binärpaket ist auf GitHub
noch nicht veröffentlicht.

Voraussetzungen: **Linux**, **Rust 1.88 oder neuer**, ein C-Linker, `pkg-config`
und die üblichen Linux-Desktop-Bibliotheken. Für das Fenster werden eine
Wayland- oder X11-Sitzung und ein OpenGL-/EGL-Treiber benötigt.

```bash
git clone https://github.com/code-by-Keyro/Task-Manager.git
cd Task-Manager
cargo run --locked
```

Für einen optimierten Build:

```bash
cargo build --release --locked
./target/release/task-manager
```

In der App lässt sich unter **Datei → Für meinen Benutzer installieren** ein
App-Menü-Eintrag einrichten. Die Benutzerinstallation benötigt keine
Administratorrechte. Alle CLI-Optionen zeigt `./target/release/task-manager --help`.

Das Skript `bash scripts/build-rust.sh` erzeugt auf Linux x86_64 ein Download-Paket
unter `release/`, einschließlich Quellarchiv, Lizenzen und SHA-256-Prüfsumme.
Dafür werden zusätzlich Node.js, `tar` und `sha256sum` benötigt.

## Sprache und Darstellung

Beim Start wird die Sprache der Linux-Sitzung erkannt. Deutsch und Englisch
sind enthalten; für weitere Sprachen gilt Englisch als Fallback. Eigene
Übersetzungen und eine feste Sprachauswahl werden in
`~/.config/linux-task-manager/languages.json` gespeichert. Persönliche Anpassungen
bleiben bei Updates erhalten. **Ansicht → Darkmode** schaltet die Darstellung um.

Anleitungen: [Deutsch](docs/Sprachen.md) · [English](docs/Languages.md).

## Plattform und Messwerte

Entwickelt und bisher auf **CachyOS/Arch Linux x86_64** geprüft. Andere
Distributionen, ARM, X11-Fensterzuordnung und einzelne Hardwarekonfigurationen
sind noch nicht vollständig verifiziert.

Linux-Messwerte stammen überwiegend aus `/proc` und `/sys`. GPU-Werte hängen
vom Treiber ab; NVIDIA verwendet zusätzlich eine vorhandene `nvidia-smi`.
Fehlende Messwerte bleiben als unbekannt erkennbar. Prozess-RAM ist RSS und
kann gemeinsam genutzten Speicher mehrfach enthalten. Docker-, Paket- und
Treiberfunktionen benötigen die passenden Werkzeuge und Berechtigungen.

## Dokumentation und Entwicklung

Der [Projekt-Vault](Vault/Start.md) enthält ausgewählte öffentliche Notizen zu
[Architektur](Vault/02%20Entwicklung/Architektur.md),
[Build und Tests](Vault/02%20Entwicklung/Build%20und%20Tests.md),
[Entscheidungen](Vault/02%20Entwicklung/Entscheidungen.md) und
[offenen Prüfungen](Vault/03%20Planung/Aufgaben.md).
Die Markdown-Dateien sind direkt auf GitHub lesbar; `Vault/` kann außerdem als
Obsidian-Vault geöffnet werden. Persönliche Installationsnotizen und reale
Rechnerberichte gehören nicht zur öffentlichen Dokumentation.

Vor Beiträgen die [Projektregeln](AGENTS.md) lesen. Nach Anwendungsänderungen:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Weitere CLI- und grafische Prüfungen beschreibt die Build-Anleitung im Vault.
Aktiver Code liegt in `src/`; `legacy/python/` enthält die Vorgängerversion
als Entwicklungsreferenz.

## Lizenz

Eigener Code: [MIT](LICENSE). Hinweise zu Drittanbieter-Bibliotheken:
[THIRD_PARTY.md](THIRD_PARTY.md) und `licenses/`.
