# Build und Tests

Im Projektordner mit Rust mindestens in der von Cargo.toml angegebenen Version:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- --check
cargo run -- --smoke-test --screenshot artifacts/smoke.png
bash scripts/build-rust.sh
```

Der GUI-Test benötigt eine grafische Linux-Sitzung. Signal-/Installationstests
verwenden selbst erstellte Wegwerfprozesse bzw. temporäre Pfade. Vorhandene
Benutzerpakete oder Container nicht zum Testen verändern. Das Download-Paket
benötigt zusätzlich Node.js, tar und sha256sum. Es enthält Quellen und Lizenzen.
Für den öffentlichen Export wurden Quellkopien, Dateiauswahl und Metadaten
geprüft; Anwendungstests wurden dafür nicht erneut ausgeführt.

## Öffentliches Anwendungspaket 0.12.1 – 12. Oktober 2026

Aus der bereinigten öffentlichen Quellfassung gebaut. fmt, Clippy mit
`-D warnings` und 61 Tests bestehen. Die Release-Anwendung besteht `--version`,
`--check` sowie echte GUI-Smoke-Tests auf Englisch/hell und Deutsch/dunkel
bei 850 × 590 Pixeln. Keine Prozesssignale oder Installationsaktionen im GUI-Test.

Das Paket enthält Anwendung, englische/deutsche Startanleitung, Quellarchiv und
Lizenznachweise für 147 aktive Abhängigkeiten. TAR-Eigentümer sind numerisch 0;
private Build-Pfade im Binary wurden remappt. Das Binärpaket benötigt Linux
x86_64 und glibc mindestens 2.43 sowie Wayland/X11 und OpenGL/EGL. Andere
Distributionen und Hardware sind damit nicht automatisch geprüft.

Download und Prüfsumme: [Release v0.12.1](https://github.com/code-by-Keyro/Task-Manager/releases/tag/v0.12.1).
Paketinhalt und eingebettete Quellen werden vor dem Upload gegen die öffentliche
Arbeitskopie verglichen; persönliche Notizen und reale Rechnerbilder bleiben ausgeschlossen.
