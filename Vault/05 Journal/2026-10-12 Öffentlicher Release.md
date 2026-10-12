# Öffentliches Anwendungspaket – 12. Oktober 2026

Version 0.12.1 als startfertiges Linux-x86_64-Paket aus den bereinigten Quellen
gebaut. Die Anwendungsversion und Rust-Logik bleiben unverändert. Die README
orientiert sich nun mit zentriertem Einstieg, Tabellen sowie Download-,
Installations- und Nutzungsbereichen an einem typischen Task-Manager-Repository.
Englisch steht weiterhin vor Deutsch; alle Funktionen gehören zu diesem Projekt.

Formatierung, Clippy und 61 Tests bestehen. Die Release-App wurde mit `--version`,
`--check` und echten GUI-Smoke-Tests auf Englisch/hell und Deutsch/dunkel geprüft.
Die Tests beenden keine Benutzerprozesse und installieren nichts.

Im Download enthalten: ausführbare Anwendung, Startanleitungen auf Englisch und
Deutsch, eigenes Quellarchiv und Drittanbieter-Lizenzen. Numerische TAR-Eigentümer
und remappte Build-Pfade vermeiden persönliche Build-Metadaten. Der Build erfordert
zur Ausführung glibc 2.43 oder neuer, eine grafische Linux-x86_64-Sitzung und
OpenGL/EGL. Kein Windows-, macOS- oder ARM-Paket.

[Anwendung und SHA-256 herunterladen](https://github.com/code-by-Keyro/Task-Manager/releases/tag/v0.12.1).
Die automatisch erzeugten „Source code“-Downloads enthalten ausschließlich Quellen.

Zurück: [Projektwissen](../Start.md).
