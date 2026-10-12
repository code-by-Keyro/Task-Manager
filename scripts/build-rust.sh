#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
    echo 'Dieses Download-Paket muss auf Linux x86_64 gebaut werden.' >&2
    exit 1
fi
if [[ -d .cargo-home ]]; then export CARGO_HOME="$PWD/.cargo-home"; fi
cargo build --release --locked
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
folder="release/Task-Manager-Rust-Linux-x86_64"
mkdir -p "$folder/licenses"
install -m 755 "${CARGO_TARGET_DIR:-target}/release/task-manager" "$folder/Task-Manager"
cp LICENSE THIRD_PARTY.md "$folder/"
cp -a licenses/. "$folder/licenses/"
metadata_file="$(mktemp)"
tree_file="$(mktemp)"
trap 'rm -f -- "$metadata_file" "$tree_file"' EXIT
cargo metadata --locked --offline --format-version 1 > "$metadata_file"
cargo tree --locked --offline --target x86_64-unknown-linux-gnu -e normal,build --prefix none --format '{p}' > "$tree_file"
node scripts/collect-licenses.mjs "$folder/licenses" "$metadata_file" "$tree_file" "$(rustc --print sysroot)"
rm -f -- "$folder"/Task-Manager-*-source.tar.gz
tar --sort=name --owner=0 --group=0 --numeric-owner --exclude='Vault/.obsidian/workspace*.json' --exclude='Vault/.obsidian/cache' --exclude='Vault/.trash' -czf "$folder/Task-Manager-${version}-source.tar.gz" \
    Cargo.toml Cargo.lock src config assets scripts licenses README.md LICENSE THIRD_PARTY.md AGENTS.md .gitignore Vault docs
cat > "$folder/START-HIER.txt" <<EOF
TASK-MANAGER $version – RUST – CACHYOS / LINUX (x86_64)

1. Entpacke dieses Paket im Dateimanager.
2. Öffne die Datei Task-Manager per Doppelklick.
   Falls nötig: Eigenschaften → Berechtigungen → als Programm ausführbar.
   Wähle bei einer Rückfrage des Dateimanagers „Ausführen“.
3. In der App: Datei → Für meinen Benutzer installieren.
4. Danach findest du Task-Manager im App-Menü.

Keine Python-, Qt- oder Rust-Installation nötig. Kein Terminal nötig.
Benötigt eine grafische Linux-Sitzung (Wayland/X11) und OpenGL/EGL.
Auf CachyOS x86_64 gebaut und getestet. Andere Distributionen nicht verifiziert.

Optional im Terminal: ./Task-Manager
Nach Installation: ~/.local/bin/task-manager
Wenn ~/.local/bin im PATH steht: task-manager
Grafische Installation: ./Task-Manager --install
Deinstallation: Datei → Deinstallieren

Sprachen: Deutsch/Englisch nach Linux-Sitzung; weitere Sprachen über
~/.config/linux-task-manager/languages.json ergänzen (App neu starten).
Englische Referenzschlüssel; en-Block als Vorlage kopieren.
Anleitungen: docs/Sprachen.md und docs/Languages.md im Quellarchiv.

Quellcode: beiliegendes source.tar.gz. Lizenzen: licenses/ und THIRD_PARTY.md.
EOF
cat > "$folder/START-HERE.txt" <<EOF
TASK-MANAGER $version – LINUX x86_64

1. Extract this archive with your file manager.
2. Double-click Task-Manager. If asked, enable executable permissions and choose Run.
3. In the app, select File → Install for my user … to add an application menu entry.

No Rust, Python or Qt installation is needed. Requires a graphical Linux session
(Wayland or X11), compatible system libraries and an OpenGL/EGL driver.
Built and tested on CachyOS/Arch Linux x86_64; other distributions are not verified.

Optional terminal start: ./Task-Manager
Show CLI options: ./Task-Manager --help
Languages: German/English; customize ~/.config/linux-task-manager/languages.json.
Source code: included source.tar.gz. Licenses: licenses/ and THIRD_PARTY.md.
EOF
archive="Task-Manager-${version}-Rust-Linux-x86_64.tar.gz"
tar --sort=name --owner=0 --group=0 --numeric-owner -czf "release/$archive" -C release "$(basename "$folder")"
(cd release && sha256sum "$archive" > "$archive.sha256")
ls -lh "release/$archive" "$folder/Task-Manager"
