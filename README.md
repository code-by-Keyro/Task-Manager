<div align="center">

<img src="assets/task-manager.svg" width="96" alt="Task-Manager icon">

# Task-Manager for Linux

A native Linux desktop task manager with a Windows 10 inspired interface, built with Rust and egui.

[![Release](https://img.shields.io/github/v/release/code-by-Keyro/Task-Manager?label=release)](https://github.com/code-by-Keyro/Task-Manager/releases/latest)
[![Platform: Linux x86_64](https://img.shields.io/badge/platform-Linux_x86__64-blue)](https://github.com/code-by-Keyro/Task-Manager/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

**[Download the application](https://github.com/code-by-Keyro/Task-Manager/releases/download/v0.12.1/Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz) · [All releases](https://github.com/code-by-Keyro/Task-Manager/releases) · [Documentation](Vault/Start.md)**

[English](#english) · [Deutsch](#deutsch)

</div>

## English

### Features

| View | What it provides |
| --- | --- |
| Processes | Applications and background processes, expandable groups, search, sorting, user filters and CPU/RAM/I/O metrics |
| Performance | Total and per-logical-processor CPU load, memory, physical disks, network, GPU, temperatures and uptime |
| Details | Individual processes, user, threads and a copyable command line |
| Docker | Containers and Compose projects, status, resource metrics, confirmed stop/restart actions |
| Optimization | Analysis of running programs and estimated CPU/RAM savings; selected termination after confirmation |
| Installed apps | Application inventory, Pacman/Flatpak metadata and confirmed uninstall actions |
| Drivers | Kernel modules, package update checks and confirmed system updates |

Light and dark themes, German/English UI and custom JSON translations are included.
Process, container and package actions require confirmation and use existing
permissions. Missing measurements stay marked as unknown.

### Download and installation

Download the **application archive** from [Releases](https://github.com/code-by-Keyro/Task-Manager/releases/latest).
GitHub's automatically generated “Source code” downloads contain sources only.

| Package | Purpose |
| --- | --- |
| [Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz](https://github.com/code-by-Keyro/Task-Manager/releases/download/v0.12.1/Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz) | Ready-to-run Linux application, licenses, start instructions and source archive |
| [SHA-256 checksum](https://github.com/code-by-Keyro/Task-Manager/releases/download/v0.12.1/Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz.sha256) | Verify the downloaded application archive |

1. Extract the application archive in your file manager.
2. Open the extracted folder and double-click **Task-Manager**. If prompted, enable
   executable permissions in the file properties and choose **Run**.
3. Optional: select **File → Install for my user …** in the app to add an application
   menu entry. Per-user installation does not require administrator permissions.

No Rust, Python or Qt installation is needed to run the downloaded application.
The prebuilt package targets **Linux x86_64 with glibc 2.43 or newer**, a Wayland/X11
session and an OpenGL/EGL driver. Built and tested on CachyOS/Arch Linux.
Other distributions and ARM binaries have not been verified.

Optional checksum check, with the archive and checksum file in the same folder:

```bash
sha256sum -c Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz.sha256
```

### Usage

From the extracted application folder:

```bash
./Task-Manager                       # Start the GUI
./Task-Manager --view cpu --dark     # Open the CPU view in dark mode
./Task-Manager --check               # Read system metrics without a window
./Task-Manager --help                # Show all CLI options
```

| Shortcut | Action |
| --- | --- |
| `F5` | Refresh |
| `Ctrl+N` | Open the run-program dialog |
| `Ctrl+Q` | Close the app when no package transaction is active |

The session language is detected at startup. Customize translations or select a
fixed language in `~/.config/linux-task-manager/languages.json`. Changes are preserved
across updates. [English guide](docs/Languages.md) · [German guide](docs/Sprachen.md).

### Platform notes

- Metrics mainly come from Linux `/proc` and `/sys`. Process memory is RSS;
  shared memory can be counted more than once.
- AMD GPU metrics use sysfs; NVIDIA also uses an existing `nvidia-smi`.
  Other drivers may provide fewer metrics. Intel GPU utilization through DRM fdinfo
  is not implemented. Hardware support depends on readable driver interfaces.
- Docker requires the existing Docker CLI and permissions. Package actions use
  Pacman/Flatpak; driver update checks require `checkupdates` from `pacman-contrib`.
  Online update checks and remote Docker contexts can use network connections.
- Window detection uses Plasma on KDE Wayland and EWMH on X11, with an estimated
  fallback. X11 detection, some NVIDIA/Intel hardware and hotplug still need real tests.
- For a distribution with older glibc, build from source in its target environment.
  Windows and macOS packages are not provided.

### Build from source

Requires Rust **1.88 or newer**, a C linker, `pkg-config` and Linux desktop libraries.

```bash
git clone https://github.com/code-by-Keyro/Task-Manager.git
cd Task-Manager
cargo build --release --locked
./target/release/task-manager
```

`bash scripts/build-rust.sh` creates the Linux-x86_64 download package under
`release/`. It also requires Node.js, `tar` and `sha256sum`.

### Documentation and contributions

Read the [project guidelines](AGENTS.md) and [build/test guide](Vault/02%20Entwicklung/Build%20und%20Tests.md).
The [public project vault](Vault/Start.md) documents architecture, decisions and
pending checks; these notes are currently in German. It can also be opened in
Obsidian. Private machine reports and deployment notes are excluded.

After application changes, run `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings` and `cargo test`.
Active code is in `src/`; the previous Python version is kept in `legacy/python/`.

### License

Original code: [MIT](LICENSE). Third-party notices: [THIRD_PARTY.md](THIRD_PARTY.md)
and the `licenses/` directory included in the application package.

---

## Deutsch

### Funktionen

| Ansicht | Inhalt |
| --- | --- |
| Prozesse | Anwendungen und Hintergrundprozesse, aufklappbare Gruppen, Suche, Sortierung, Benutzerfilter und CPU/RAM/E/A |
| Leistung | CPU-Gesamtlast und logische Prozessoren, RAM, physische Laufwerke, Netzwerk, GPU, Temperaturen und Laufzeit |
| Details | Einzelne Prozesse, Benutzer, Threads und kopierbare Befehlszeile |
| Docker | Container und Compose-Projekte, Status, Ressourcen und bestätigtes Stoppen/Neustarten |
| Optimierung | Analyse laufender Programme und geschätztes CPU/RAM-Potenzial; ausgewähltes Beenden mit Rückfrage |
| Installierte Apps | App-Inventar, Pacman/Flatpak-Metadaten und bestätigte Deinstallation |
| Treiber | Kernelmodule, Suche nach Paketupdates und bestätigte Systemaktualisierung |

Helles Design, Darkmode, Deutsch/Englisch und eigene JSON-Übersetzungen sind enthalten.
Prozess-, Container- und Paketaktionen benötigen eine Bestätigung und vorhandene
Berechtigungen. Fehlende Messwerte bleiben als unbekannt erkennbar.

### Download und Installation

Das **Anwendungspaket** steht unter [Releases](https://github.com/code-by-Keyro/Task-Manager/releases/latest).
GitHubs automatisch erzeugte „Source code“-Downloads enthalten nur die Quellen.

| Paket | Zweck |
| --- | --- |
| [Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz](https://github.com/code-by-Keyro/Task-Manager/releases/download/v0.12.1/Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz) | Startfertige Linux-Anwendung, Lizenzen, Startanleitungen und Quellarchiv |
| [SHA-256-Prüfsumme](https://github.com/code-by-Keyro/Task-Manager/releases/download/v0.12.1/Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz.sha256) | Heruntergeladenes Anwendungspaket prüfen |

1. Das Anwendungspaket im Dateimanager entpacken.
2. Den entpackten Ordner öffnen und **Task-Manager** doppelklicken. Falls nötig:
   Eigenschaften → Berechtigungen → als Programm ausführbar; **Ausführen** wählen.
3. Optional in der App **Datei → Für meinen Benutzer installieren** wählen.
   Die Benutzerinstallation legt einen App-Menü-Eintrag an und benötigt keine
   Administratorrechte.

Zum Starten der fertigen Anwendung werden weder Rust, Python noch Qt benötigt.
Das Binärpaket benötigt **Linux x86_64 mit glibc 2.43 oder neuer**, eine Wayland/X11-
Sitzung und einen OpenGL/EGL-Treiber. Auf CachyOS/Arch Linux gebaut und getestet.
Andere Distributionen und ARM-Binärpakete sind nicht verifiziert.

Optionale Prüfsummenkontrolle; Paket und Prüfsummendatei liegen im selben Ordner:

```bash
sha256sum -c Task-Manager-0.12.1-Rust-Linux-x86_64.tar.gz.sha256
```

### Nutzung

Im entpackten Anwendungsordner:

```bash
./Task-Manager                       # Oberfläche starten
./Task-Manager --view cpu --dark     # CPU-Ansicht im Darkmode öffnen
./Task-Manager --check               # Systemmesswerte ohne Fenster lesen
./Task-Manager --help                # Alle CLI-Optionen anzeigen
```

| Tastenkürzel | Aktion |
| --- | --- |
| `F5` | Aktualisieren |
| `Strg+N` | Dialog zum Starten eines Programms öffnen |
| `Strg+Q` | App schließen, wenn keine Pakettransaktion aktiv ist |

Beim Start wird die Sitzungssprache erkannt. Übersetzungen und feste Sprachwahl
stehen in `~/.config/linux-task-manager/languages.json`; Anpassungen bleiben bei
Updates erhalten. [Deutsche Anleitung](docs/Sprachen.md) · [English guide](docs/Languages.md).

### Plattformhinweise

- Messwerte stammen überwiegend aus `/proc` und `/sys`. Prozess-RAM ist RSS;
  gemeinsam genutzter Speicher kann mehrfach gezählt werden.
- AMD-GPU-Werte kommen aus sysfs, NVIDIA verwendet zusätzlich `nvidia-smi`.
  Andere Treiber liefern gegebenenfalls weniger Werte. Intel-GPU-Auslastung über
  DRM-fdinfo ist nicht implementiert. Unterstützung hängt von lesbaren Treiberwerten ab.
- Docker benötigt Docker CLI und Berechtigungen. Paketaktionen verwenden
  Pacman/Flatpak, Treiber-Updateprüfungen `checkupdates` aus `pacman-contrib`.
  Online-Updateprüfungen und entfernte Docker-Kontexte können Netzwerkzugriffe ausführen.
- Fenstererkennung über Plasma unter KDE Wayland, EWMH unter X11 und geschätzten
  Fallback. X11, bestimmte NVIDIA/Intel-Hardware und Hotplug benötigen weitere reale Tests.
- Bei älterer glibc aus den Quellen in der Zielumgebung bauen.
  Windows- und macOS-Pakete werden nicht angeboten.

### Aus Quellen bauen

Rust **1.88 oder neuer**, C-Linker, `pkg-config` und Linux-Desktop-Bibliotheken:

```bash
git clone https://github.com/code-by-Keyro/Task-Manager.git
cd Task-Manager
cargo build --release --locked
./target/release/task-manager
```

`bash scripts/build-rust.sh` erzeugt das Linux-x86_64-Paket unter `release/`.
Zusätzlich werden Node.js, `tar` und `sha256sum` benötigt.

### Dokumentation und Beiträge

[Projektregeln](AGENTS.md) und [Build-/Testanleitung](Vault/02%20Entwicklung/Build%20und%20Tests.md)
lesen. Der [öffentliche Vault](Vault/Start.md) dokumentiert Architektur,
Entscheidungen und offene Prüfungen und kann in Obsidian geöffnet werden.
Persönliche Rechnerberichte und Installationsnotizen bleiben ausgeschlossen.

Nach Anwendungsänderungen `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings` und `cargo test` ausführen.
Aktiver Code liegt in `src/`, der Python-Vorgänger in `legacy/python/`.

### Lizenz

Eigener Code: [MIT](LICENSE). Drittanbieter-Hinweise: [THIRD_PARTY.md](THIRD_PARTY.md)
und das Verzeichnis `licenses/` im Anwendungspaket.
