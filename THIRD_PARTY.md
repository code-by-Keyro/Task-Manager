# Bibliotheken der Rust-Version

Der eigene Anwendungscode steht unter der MIT-Lizenz. Die Rust-Anwendung verwendet
egui/eframe und egui_extras (MIT/Apache-2.0), libc (MIT/Apache-2.0), png
(MIT/Apache-2.0) und shell-words (MIT/Apache-2.0) sowie deren Abhängigkeiten.
Die genauen Versionen sind in `Cargo.lock` festgehalten.

Das Download-Paket enthält unter `licenses/` die Lizenzdateien der im Linux-Build
verwendeten Cargo-Pakete und eine Paketliste mit Versions- und Lizenzangaben.
Schriftarten und die Rust-Standardbibliothek behalten ihre jeweiligen Lizenzen.
Die Fonts-Lizenzen sind im Paket `epaint_default_fonts` enthalten.

Die Anwendung benötigt die üblichen Linux-Systembibliotheken und einen
OpenGL-/EGL-Treiber für die grafische Sitzung. Python und Qt werden nicht verwendet.
Es besteht keine Verbindung zu Microsoft oder Windows.

Die frühere Python-Version und ihre Lizenzhinweise liegen ausschließlich als
Entwicklungsreferenz in `legacy/python/` und sind nicht Teil des Rust-Downloads.
