<img src="assets/task-manager.svg" alt="Task-Manager" width="64">

# Task-Manager for Linux

[English](#english) · [Deutsch](#deutsch)

## English

A native desktop task manager for Linux, written in **Rust** with
**egui/eframe**. Monitor processes, hardware and system performance in a
Windows 10 inspired interface, with dark mode and configurable translations.

**Version: 0.12.1 · Linux x86_64 · MIT license**

### Features

- **Processes:** Applications and background processes, search, sorting,
  user filters, CPU/RAM/I/O metrics and process details.
- **Performance:** Total CPU load and individual logical processors, memory,
  GPU, physical disks, networking, temperatures and system uptime.
- **Docker:** Containers and Compose projects with resource metrics; confirmed
  stop and restart actions through the existing Docker CLI.
- **Optimization:** Analyze running programs and estimate potential CPU/RAM
  savings; terminate selected processes only after confirmation.
- **Apps and drivers:** View installed applications and kernel modules;
  Pacman/Flatpak package actions and confirmed system updates.
- **Appearance:** Light interface, saved dark mode, German and English,
  and custom translations through JSON.

Process, container and package actions use existing user permissions and
require confirmation. The app does not perform these actions automatically.

### Build and run

This repository contains the source code. A prebuilt binary package has
not yet been published on GitHub.

Requirements: **Linux**, **Rust 1.88 or newer**, a C linker, `pkg-config`
and the usual Linux desktop libraries. The window requires a Wayland or X11
session and an OpenGL/EGL driver.

```bash
git clone https://github.com/code-by-Keyro/Task-Manager.git
cd Task-Manager
cargo run --locked
```

For an optimized build:

```bash
cargo build --release --locked
./target/release/task-manager
```

Choose **File → Install for my user …** in the app to add an application menu
entry. Per-user installation does not require administrator permissions.
Run `./target/release/task-manager --help` for all CLI options.

On Linux x86_64, `bash scripts/build-rust.sh` creates a download package in
`release/`, including a source archive, licenses and a SHA-256 checksum.
This also requires Node.js, `tar` and `sha256sum`.

### Language and appearance

The app detects the language of the Linux session at startup. German and
English are included; English is the fallback for other languages. Custom
translations and a fixed language selection are stored in
`~/.config/linux-task-manager/languages.json`. Personal changes are preserved
across updates. Use **View → Dark mode** to switch the appearance.

Translation guides: [English](docs/Languages.md) · [Deutsch](docs/Sprachen.md).

### Platform and metrics

Developed and tested so far on **CachyOS/Arch Linux x86_64**. Other distributions,
ARM, X11 window detection and some hardware configurations have not yet been
fully verified.

Linux metrics mainly come from `/proc` and `/sys`. GPU metrics depend on the
driver; NVIDIA also uses an existing `nvidia-smi` installation. Missing metrics
remain marked as unknown. Process RAM is RSS and may count shared memory more
than once. Docker, package and driver features require the appropriate tools
and permissions.

### Documentation and development

The [project vault](Vault/Start.md) contains selected public notes on
[architecture](Vault/02%20Entwicklung/Architektur.md),
[builds and tests](Vault/02%20Entwicklung/Build%20und%20Tests.md),
[decisions](Vault/02%20Entwicklung/Entscheidungen.md) and
[pending checks](Vault/03%20Planung/Aufgaben.md).
These notes are currently in German. The Markdown files can be read directly
on GitHub; `Vault/` can also be opened as an Obsidian vault. Personal installation
notes and actual machine reports are kept outside the public documentation.

Read the [project guidelines](AGENTS.md) before contributing. After application
changes, run:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The build guide in the vault describes additional CLI and graphical checks.
Active code is in `src/`; `legacy/python/` contains the previous version
as a development reference.

### License

Original code: [MIT](LICENSE). Third-party library information:
[THIRD_PARTY.md](THIRD_PARTY.md) and `licenses/`.

---

## Deutsch

Ein nativer Desktop-Task-Manager für Linux, geschrieben in **Rust** mit
**egui/eframe**. Prozesse, Hardware und Systemleistung in einer Oberfläche im
Stil von Windows 10 – mit Darkmode und konfigurierbaren Übersetzungen.

**Stand: 0.12.1 · Linux x86_64 · MIT-Lizenz**

### Funktionen

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

### Bauen und starten

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

### Sprache und Darstellung

Beim Start wird die Sprache der Linux-Sitzung erkannt. Deutsch und Englisch
sind enthalten; für weitere Sprachen gilt Englisch als Fallback. Eigene
Übersetzungen und eine feste Sprachauswahl werden in
`~/.config/linux-task-manager/languages.json` gespeichert. Persönliche Anpassungen
bleiben bei Updates erhalten. **Ansicht → Darkmode** schaltet die Darstellung um.

Anleitungen: [Deutsch](docs/Sprachen.md) · [English](docs/Languages.md).

### Plattform und Messwerte

Entwickelt und bisher auf **CachyOS/Arch Linux x86_64** geprüft. Andere
Distributionen, ARM, X11-Fensterzuordnung und einzelne Hardwarekonfigurationen
sind noch nicht vollständig verifiziert.

Linux-Messwerte stammen überwiegend aus `/proc` und `/sys`. GPU-Werte hängen
vom Treiber ab; NVIDIA verwendet zusätzlich eine vorhandene `nvidia-smi`.
Fehlende Messwerte bleiben als unbekannt erkennbar. Prozess-RAM ist RSS und
kann gemeinsam genutzten Speicher mehrfach enthalten. Docker-, Paket- und
Treiberfunktionen benötigen die passenden Werkzeuge und Berechtigungen.

### Dokumentation und Entwicklung

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

### Lizenz

Eigener Code: [MIT](LICENSE). Hinweise zu Drittanbieter-Bibliotheken:
[THIRD_PARTY.md](THIRD_PARTY.md) und `licenses/`.
