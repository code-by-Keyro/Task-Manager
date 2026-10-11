# Enthaltene Komponenten

Die MIT-Lizenz gilt für den eigenen App-Code. Gebündelte Bibliotheken behalten
ihre eigenen Lizenzen. Die App hat keine Verbindung zu Microsoft oder Windows.

- Python: PSF License, https://www.python.org/psf/license/
- Qt 6, PySide6 und Shiboken6: LGPLv3/GPLv3 bzw. die Lizenzen der einzelnen Qt-Komponenten,
  https://doc.qt.io/qt-6/licenses-used-in-qt.html und https://code.qt.io/cgit/pyside/pyside-setup.git/
- psutil: BSD 3-Clause, https://github.com/giampaolo/psutil
- PyInstaller-Bootloader: GPL mit Ausnahme für gebündelte Anwendungen,
  https://pyinstaller.org/en/stable/license.html

Das Download-Paket enthält die Lizenztexte der Python-Abhängigkeiten im Ordner
`licenses` sowie den vollständigen eigenen Quellcode im `source.tar.gz`.
Die verwendeten Python-Paketversionen stehen in `requirements-lock.txt`.

Qt/PySide6 bleiben austauschbare dynamische Bibliotheken. Um die Anwendung mit
einer geänderten Qt/PySide6-Version zu verwenden, entpacke den App-Quellcode,
installiere deine kompatible Bibliotheksversion in einer virtuellen Umgebung
und starte `python -m task_manager` oder baue mit `python scripts/build.py`
eine neue ausführbare Datei. Debugging und Änderungen an LGPL-Komponenten
sind gestattet. Quellcode der Bibliotheken: https://download.qt.io/archive/qt/
und https://download.qt.io/official_releases/QtForPython/ .
