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
