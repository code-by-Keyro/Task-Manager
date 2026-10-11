#!/usr/bin/env python3
"""Build a portable, self-contained Linux download and source archive."""
import hashlib
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from task_manager import __version__


def archive_sources(target):
    with tarfile.open(target, "w:gz") as archive:
        for name in ("task_manager", "scripts", "tests", "licenses", "pyproject.toml", "requirements-lock.txt",
                     "launcher.py", "README.md", "LICENSE", "THIRD_PARTY.md"):
            path = ROOT / name
            candidates = sorted(path.rglob("*")) if path.is_dir() else [path]
            for candidate in candidates:
                if candidate.is_file() and "__pycache__" not in candidate.parts and candidate.suffix != ".pyc":
                    archive.add(candidate, arcname="Task-Manager-source/" + str(candidate.relative_to(ROOT)))


def main():
    if sys.platform != "linux" or platform.machine() != "x86_64":
        raise SystemExit("Dieses Download-Paket muss auf Linux x86_64 gebaut werden.")
    subprocess.run([sys.executable, "-m", "PyInstaller", "--clean", "--noconfirm", "--onefile",
        "--name", "Task-Manager", "--add-data", "task_manager/assets:task_manager/assets",
        "--exclude-module", "PySide6.QtWebEngineCore", "--exclude-module", "PySide6.QtWebEngineWidgets",
        "--exclude-module", "tkinter", "launcher.py"], cwd=ROOT, check=True)
    release = ROOT / "release"
    release.mkdir(exist_ok=True)
    folder = release / "Task-Manager-Linux-x86_64"
    folder.mkdir(exist_ok=True)
    binary = folder / "Task-Manager"
    shutil.copy2(ROOT / "dist/Task-Manager", binary)
    binary.chmod(0o755)
    for name in ("LICENSE", "THIRD_PARTY.md"):
        shutil.copy2(ROOT / name, folder / name)
    shutil.copytree(ROOT / "licenses", folder / "licenses", dirs_exist_ok=True)
    source = folder / f"Task-Manager-{__version__}-source.tar.gz"
    archive_sources(source)
    (folder / "START-HIER.txt").write_text(
        f"TASK-MANAGER {__version__} – CACHYOS / ARCH LINUX (x86_64)\n\n"
        "1. Entpacke das Download-Paket im Dateimanager.\n"
        "2. Öffne die Datei Task-Manager per Doppelklick.\n"
        "   Falls nötig: Rechtsklick → Eigenschaften → Berechtigungen → als Programm ausführbar.\n"
        "   Wähle bei einer Rückfrage des Dateimanagers „Ausführen“.\n"
        "3. In der App: Datei → Für meinen Benutzer installieren.\n"
        "4. Anschließend findest du Task-Manager im App-Menü.\n\n"
        "Python und Qt sind enthalten. Es wird kein Terminal benötigt.\n"
        "Die App benötigt eine grafische Linux-Sitzung (Wayland oder X11).\n"
        "Das Paket wurde auf CachyOS gebaut und dort geprüft.\n"
        "Andere Linux-Distributionen sind nicht als kompatibel verifiziert.\n\n"
        "Terminal (optional): ./Task-Manager\n"
        "Nach Installation: ~/.local/bin/task-manager\n"
        "Wenn ~/.local/bin im PATH steht: task-manager\n"
        "Grafische Installation: ./Task-Manager --install\n"
        "Deinstallation: Datei → Deinstallieren\n\n"
        "Quellcode und Bauanleitung: beiliegendes source.tar.gz.\n",
        encoding="utf-8")
    output = release / f"Task-Manager-{__version__}-Linux-x86_64.tar.gz"
    with tarfile.open(output, "w:gz") as archive:
        archive.add(folder, arcname=folder.name)
    checksum = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_suffix(output.suffix + ".sha256").write_text(f"{checksum}  {output.name}\n")
    print(f"Download: {output}\nSHA256: {checksum}")


if __name__ == "__main__":
    main()
