# Projektübersicht

Nativer Linux-Task-Manager mit Rust und egui/eframe für CachyOS/Arch Linux x86_64.
Windows-10-artige helle Oberfläche, optionaler Darkmode, Deutsch und Englisch
mit Englisch-Fallback. Zusätzliche Übersetzungen sind per JSON möglich.

## Ansichten und Aktionen

Prozesse, Leistung, Hardware, Docker, Optimierung, Apps und Treiber haben eigene
Ansichten. Bestätigte Prozess-, Container- und Paketaktionen prüfen ihre Ziele.
Prozesssignale verwenden pidfd und eine Startzeitprüfung, um eine inzwischen
wiederverwendete PID nicht als dasselbe Ziel zu behandeln.

Fenster-PIDs stammen auf KDE Wayland aus dem Plasma-Fensterprotokoll, auf X11
aus EWMH. Ohne lesbare Fensterliste verwendet die App Desktop-Einträge und
kennzeichnet die Zuordnung als geschätzt. Anwendungskategorien sind keine
automatische Prozessfamilienhierarchie.

## Bedeutung und Grenzen der Messwerte

- Prozess-CPU ist auf die gesamte CPU-Kapazität normalisiert. Die erste Messung
  liefert eine Baseline für CPU- und E/A-Raten.
- Prozess-RAM ist RSS; gemeinsam genutzte Seiten können mehrfach gezählt werden.
  Die Gesamtbelegung verwendet `MemTotal - MemAvailable`.
- RAM-Cache enthält `Cached + SReclaimable`; er wird nicht zusätzlich zur
  Gesamtbelegung addiert.
- Physische Datenträger werden ohne Partitionen und virtuelle/RAM-Geräte angezeigt.
  Netzwerkschnittstellen werden ohne Loopback summiert; Bridges/VPNs können
  Daten mehrfach zählen.
- GPU-Werte hängen von lesbaren Treiberschnittstellen ab. AMD verwendet sysfs,
  NVIDIA zusätzlich eine vorhandene `nvidia-smi`. Intel-GPU-Auslastung über
  DRM-fdinfo und SATA-SMART-Abfragen sind nicht implementiert.
- Optimierungspotenzial ist eine Schätzung: RSS als RAM-Obergrenze und CPU als
  Momentaufnahme. Tatsächliche Einsparung hängt von der weiteren Nutzung ab.

Ein unbekannter Wert wird nicht als Null dargestellt. Prozessrechte werden vom
Kernel geprüft. Docker-/Paketaktionen nutzen vorhandene Werkzeuge und Rechte.

Start und Funktionsübersicht: [README](../../README.md).
Sprachen: [Deutsch](../../docs/Sprachen.md) · [English](../../docs/Languages.md).
