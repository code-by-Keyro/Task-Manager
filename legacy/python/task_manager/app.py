import argparse
import os
from pathlib import Path
import platform
import shlex
import sys
import threading

import psutil
from PySide6.QtCore import Qt, QThread, Signal, QTimer, QSettings, QProcess
from PySide6.QtGui import QAction, QActionGroup, QIcon, QKeySequence
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, QLabel,
    QTabWidget, QTableView, QHeaderView, QLineEdit, QCheckBox, QPushButton,
    QAbstractItemView, QMessageBox, QMenu, QInputDialog, QButtonGroup,
    QStackedWidget, QGridLayout, QFrame,
)

from . import __version__
from .charts import Chart, ResourceButton
from .install import APP_ID, install_dialog, uninstall_dialog
from .model import ProcessModel, ProcessFilter, ResourceDelegate, ROW_ROLE, decimal, size
from .monitor import Sampler, Snapshot, cpu_name, distribution, end_process

ICON_PATH = Path(__file__).parent / "assets/task-manager.svg"
STYLE = """
QMainWindow, QWidget { background: #fff; color: #202832; font-family: 'Segoe UI', 'Noto Sans', sans-serif; font-size: 10pt; }
QMenuBar { border-bottom: 1px solid #e4e8ec; padding: 2px 7px; }
QMenuBar::item { padding: 5px 9px; }
QMenuBar::item:selected, QMenu::item:selected { background: #e7f2fa; }
QMenu { border: 1px solid #d0d6dc; padding: 5px; }
QMenu::item { padding: 7px 22px; }
QTabWidget::pane { border: 0; border-top: 1px solid #d9dfe5; }
QTabBar::tab { padding: 11px 20px; border-bottom: 3px solid transparent; color: #56616b; }
QTabBar::tab:selected { color: #0078d7; border-bottom: 3px solid #0078d7; }
QTabBar::tab:hover { background: #f4f8fb; }
QLineEdit { padding: 7px 11px; border: 1px solid #c6cdd4; border-radius: 3px; selection-background-color: #0078d7; }
QLineEdit:focus { border: 1px solid #0078d7; }
QCheckBox { spacing: 7px; }
QCheckBox::indicator { width: 14px; height: 14px; border: 1px solid #aebbc6; border-radius: 2px; background: white; }
QCheckBox::indicator:hover { border-color: #0078d7; }
QCheckBox::indicator:checked { background: #0078d7; border-color: #0078d7; }
QTableView { border: 0; gridline-color: #edf0f3; selection-background-color: #dceefc; selection-color: #182f43; }
QTableView::item { padding: 0 10px; border-bottom: 1px solid #f2f4f6; }
QTableView::item:selected { background: #dceefc; color: #182f43; }
QHeaderView::section { background: #fafbfc; border: 0; border-right: 1px solid #e5e9ec; border-bottom: 1px solid #dce2e7; padding: 9px; font-weight: 400; }
QPushButton { border: 1px solid #b8c1c9; padding: 7px 15px; border-radius: 3px; background: #fafbfc; }
QPushButton:hover { background: #eaf3fb; border-color: #0078d7; }
QPushButton:pressed { background: #d4e8f8; }
QPushButton:disabled { color: #9ca5ad; border-color: #dde2e6; background: #f7f8f9; }
QPushButton#endTask { color: #fff; background: #0078d7; border-color: #0078d7; }
QPushButton#endTask:hover { background: #006bbf; }
QPushButton#endTask:disabled { color: #9ca5ad; background: #f0f2f4; border-color: #dce2e7; }
QLabel#muted { color: #647585; font-size: 9pt; }
QLabel#title { font-size: 23pt; font-weight: 400; }
QLabel#metric { font-size: 24pt; color: #0078d7; }
QStatusBar { border-top: 1px solid #dce2e7; background: #fafbfc; }
QStatusBar QLabel { background: transparent; padding: 5px 8px; font-size: 9pt; }
QScrollBar:vertical { background: #f6f8fa; width: 12px; }
QScrollBar::handle:vertical { background: #c9d1d8; min-height: 30px; border: 3px solid #f6f8fa; border-radius: 5px; }
QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical { height: 0; }
QScrollBar::add-page:vertical, QScrollBar::sub-page:vertical { background: none; }
"""


def label(text="", name=None):
    widget = QLabel(text)
    if name:
        widget.setObjectName(name)
    return widget


class MonitorThread(QThread):
    updated = Signal(object)
    failed = Signal(str)

    def __init__(self, interval=1.0, parent=None):
        super().__init__(parent)
        self.interval = interval
        self.paused = False
        self.wake = threading.Event()

    def run(self):
        try:
            sampler = Sampler()
        except Exception as error:
            self.failed.emit(str(error))
            return
        while not self.isInterruptionRequested():
            if not self.paused:
                try:
                    self.updated.emit(sampler.sample())
                except Exception as error:
                    self.failed.emit(str(error))
            self.wake.wait(self.interval)
            self.wake.clear()

    def stop(self):
        self.requestInterruption()
        self.wake.set()
        self.wait()


class TaskWindow(QMainWindow):
    def __init__(self, settings_enabled=True):
        super().__init__()
        self.settings = QSettings(APP_ID, "Task-Manager") if settings_enabled else None
        self.setWindowTitle("Task-Manager")
        self.setWindowIcon(QIcon(str(ICON_PATH)))
        self.resize(1100, 740)
        self.setMinimumSize(850, 590)
        self.snapshot = None
        self.samples = 0
        self.tabs = QTabWidget()
        self.setCentralWidget(self.tabs)
        self.tables = []
        self.models = []
        self.filters = []
        self.searches = []
        self.mine_checks = []
        self.end_buttons = []
        self._menus()
        self._process_page(False)
        self._performance_page()
        self._process_page(True)
        self._system_page()
        self.tabs.currentChanged.connect(self._tab_changed)
        self.summary = label("System wird eingelesen …")
        self.live = label("●  Live · 1 s")
        self.live.setStyleSheet("color: #207957;")
        self.statusBar().addWidget(self.summary, 1)
        self.statusBar().addPermanentWidget(self.live)
        self.worker = MonitorThread(parent=self)
        self.worker.updated.connect(self._update)
        self.worker.failed.connect(self._error)
        self.worker.start()
        if self.settings:
            geometry = self.settings.value("geometry")
            if geometry:
                self.restoreGeometry(geometry)
            if self.settings.value("always_on_top", False, type=bool):
                self.top_action.setChecked(True)

    def _menus(self):
        file_menu = self.menuBar().addMenu("Datei")
        new_task = file_menu.addAction("Neuen Task ausführen …")
        new_task.setShortcut("Ctrl+N")
        new_task.triggered.connect(self._new_task)
        file_menu.addSeparator()
        file_menu.addAction("Für meinen Benutzer installieren …", lambda: install_dialog(self, ICON_PATH))
        file_menu.addAction("Deinstallieren …", lambda: uninstall_dialog(self))
        file_menu.addSeparator()
        exit_action = file_menu.addAction("Beenden", self.close)
        exit_action.setShortcut("Ctrl+Q")
        options = self.menuBar().addMenu("Optionen")
        self.top_action = options.addAction("Immer im Vordergrund")
        self.top_action.setCheckable(True)
        self.top_action.toggled.connect(self._top)
        view = self.menuBar().addMenu("Ansicht")
        refresh = view.addAction("Jetzt aktualisieren", self._refresh)
        refresh.setShortcut(QKeySequence("F5"))
        self.pause_action = view.addAction("Aktualisierung pausieren")
        self.pause_action.setCheckable(True)
        self.pause_action.toggled.connect(self._pause)
        interval_menu = view.addMenu("Aktualisierungsgeschwindigkeit")
        group = QActionGroup(self)
        for text, seconds in (("Hoch (0,5 s)", 0.5), ("Normal (1 s)", 1.0), ("Niedrig (2 s)", 2.0)):
            action = QAction(text, self, checkable=True)
            group.addAction(action)
            interval_menu.addAction(action)
            action.setChecked(seconds == 1.0)
            action.triggered.connect(lambda checked, value=seconds: self._interval(value))
        help_menu = self.menuBar().addMenu("Hilfe")
        help_menu.addAction("Über Task-Manager", self._about)

    def _process_page(self, detailed):
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)
        tools = QHBoxLayout()
        tools.setContentsMargins(16, 14, 16, 14)
        search = QLineEdit()
        search.setPlaceholderText("Prozesse durchsuchen · Name, PID oder Benutzer")
        search.setClearButtonEnabled(True)
        search.setMaximumWidth(450)
        mine = QCheckBox("Nur meine Prozesse")
        tools.addWidget(search, 1)
        tools.addStretch()
        tools.addWidget(mine)
        layout.addLayout(tools)
        model = ProcessModel(detailed, self)
        proxy = ProcessFilter(self)
        proxy.setSourceModel(model)
        search.textChanged.connect(proxy.set_query)
        mine.toggled.connect(proxy.set_only_mine)
        table = QTableView()
        table.setModel(proxy)
        for column in ([4, 5] if detailed else [2, 3, 4]):
            table.setItemDelegateForColumn(column, ResourceDelegate(table))
        table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        table.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        table.setSortingEnabled(True)
        table.sortByColumn(4 if detailed else 2, Qt.SortOrder.DescendingOrder)
        table.verticalHeader().hide()
        table.verticalHeader().setDefaultSectionSize(34)
        table.setShowGrid(False)
        table.setAlternatingRowColors(False)
        table.setWordWrap(False)
        table.setHorizontalScrollMode(QAbstractItemView.ScrollMode.ScrollPerPixel)
        header = table.horizontalHeader()
        header.setMinimumSectionSize(70)
        header.setStretchLastSection(detailed)
        widths = [235, 75, 125, 140, 85, 140, 80, 350] if detailed else [330, 180, 100, 170, 170, 90]
        for index, width in enumerate(widths):
            table.setColumnWidth(index, width)
        if not detailed:
            header.setSectionResizeMode(0, QHeaderView.ResizeMode.Stretch)
        table.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        table.customContextMenuRequested.connect(lambda point, view=table: self._context(view, point))
        layout.addWidget(table)
        bottom = QHBoxLayout()
        bottom.setContentsMargins(16, 10, 16, 12)
        hint = label("CPU: Anteil am gesamten System   ·   RAM: residenter Speicher (RSS)", "muted")
        hint.setWordWrap(True)
        bottom.addWidget(hint, 1)
        end = QPushButton("Task beenden")
        end.setObjectName("endTask")
        end.setEnabled(False)
        end.clicked.connect(lambda checked=False, view=table: self._end(view))
        bottom.addWidget(end)
        table.selectionModel().selectionChanged.connect(lambda selected, deselected, view=table, button=end:
                                                         button.setEnabled(self._selected(view) is not None))
        layout.addLayout(bottom)
        self.tabs.addTab(page, "Details" if detailed else "Prozesse")
        self.tables.append(table)
        self.models.append(model)
        self.filters.append(proxy)
        self.searches.append(search)
        self.mine_checks.append(mine)
        self.end_buttons.append(end)

    def _performance_page(self):
        page = QWidget()
        layout = QHBoxLayout(page)
        layout.setContentsMargins(12, 20, 24, 20)
        layout.setSpacing(25)
        sidebar = QVBoxLayout()
        sidebar.setSpacing(4)
        sidebar.setContentsMargins(0, 0, 0, 0)
        self.resources = []
        self.resource_group = QButtonGroup(self)
        self.performance_stack = QStackedWidget()
        self.charts, self.metrics, self.stats, self.scales = [], [], [], []
        colors = ["#1485c5", "#9460b8", "#579b30", "#bd7850"]
        titles = ["CPU", "Arbeitsspeicher", "Datenträger", "Netzwerk"]
        subtitles = [cpu_name(), "Physischer Speicher", "Alle Datenträger · Lese- und Schreibrate",
                     "Alle Netzwerkschnittstellen ohne Loopback"]
        for number, (title, color, subtitle) in enumerate(zip(titles, colors, subtitles)):
            button = ResourceButton(title, color)
            self.resource_group.addButton(button, number)
            button.clicked.connect(lambda checked, index=number: self.performance_stack.setCurrentIndex(index))
            self.resources.append(button)
            sidebar.addWidget(button)
            content = QWidget()
            right = QVBoxLayout(content)
            right.setContentsMargins(0, 0, 0, 0)
            heading = QHBoxLayout()
            heading.addWidget(label(title, "title"))
            metric = label("—", "metric")
            metric.setStyleSheet(f"color: {color}; font-size: 24pt;")
            heading.addStretch()
            heading.addWidget(metric)
            self.metrics.append(metric)
            right.addLayout(heading)
            sublabel = label(subtitle, "muted")
            sublabel.setWordWrap(True)
            right.addWidget(sublabel)
            right.addSpacing(16)
            scale = label("100 %" if number < 2 else "", "muted")
            scale.setAlignment(Qt.AlignmentFlag.AlignRight)
            self.scales.append(scale)
            right.addWidget(scale)
            chart = Chart(color)
            self.charts.append(chart)
            right.addWidget(chart, 1)
            captions = QHBoxLayout()
            captions.addWidget(label("Letzte 60 Messungen", "muted"))
            captions.addStretch()
            captions.addWidget(label("Jetzt", "muted"))
            right.addLayout(captions)
            if number == 0:
                self.per_core = QCheckBox("Logische Prozessoren einzeln anzeigen")
                self.per_core.setToolTip("Beim Umschalten beginnt der Verlauf neu. Jeder Kern hat eine eigene Farbe.")
                right.addWidget(self.per_core)
            else:
                legend = ("Speichernutzung ohne verfügbaren Cache" if number == 1 else
                          "Grün: Lesen · Violett: Schreiben" if number == 2 else
                          "Braun: Empfangen · Violett: Senden")
                right.addWidget(label(legend, "muted"))
            right.addSpacing(16)
            stats = label("Messwerte werden geladen …")
            stats.setWordWrap(True)
            stats.setMinimumHeight(76)
            self.stats.append(stats)
            right.addWidget(stats)
            self.performance_stack.addWidget(content)
        self.resources[0].setChecked(True)
        sidebar.addStretch()
        sidebar.addWidget(label("Direkt vom Linux-System\nKeine Administratorrechte nötig", "muted"))
        layout.addLayout(sidebar)
        line = QFrame()
        line.setFrameShape(QFrame.Shape.VLine)
        line.setStyleSheet("color: #e0e6eb;")
        layout.addWidget(line)
        layout.addWidget(self.performance_stack, 1)
        self.tabs.addTab(page, "Leistung")

    def _system_page(self):
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(32, 26, 32, 26)
        layout.addWidget(label("Dein System", "title"))
        layout.addWidget(label("Hardware und Linux auf einen Blick", "muted"))
        layout.addSpacing(25)
        grid = QGridLayout()
        grid.setHorizontalSpacing(40)
        grid.setVerticalSpacing(16)
        try:
            physical = psutil.cpu_count(logical=False)
        except Exception:
            physical = None
        for row, (key, value) in enumerate((
            ("Betriebssystem", distribution()), ("Kernel", platform.release()),
            ("Architektur", platform.machine()), ("Prozessor", cpu_name()),
            ("CPU-Kerne", str(physical or "—")), ("Logische Prozessoren", str(psutil.cpu_count() or "—")),
            ("Arbeitsspeicher", size(psutil.virtual_memory().total)),
            ("Computername", platform.node()), ("App-Version", __version__),
        )):
            grid.addWidget(label(key, "muted"), row, 0)
            value_label = label(value)
            value_label.setWordWrap(True)
            value_label.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
            grid.addWidget(value_label, row, 1)
        grid.setColumnStretch(1, 1)
        layout.addLayout(grid)
        layout.addStretch()
        self.system_live = label()
        layout.addWidget(self.system_live)
        install = QPushButton("Im App-Menü installieren …")
        install.clicked.connect(lambda: install_dialog(self, ICON_PATH))
        layout.addWidget(install, alignment=Qt.AlignmentFlag.AlignLeft)
        self.tabs.addTab(page, "System")

    @staticmethod
    def _selected(table):
        rows = table.selectionModel().selectedRows()
        return rows[0].data(ROW_ROLE) if rows else None

    def _update(self, snapshot: Snapshot):
        self.snapshot = snapshot
        self.samples += 1
        for table, model, proxy, button in zip(self.tables, self.models, self.filters, self.end_buttons):
            selected = self._selected(table)
            scroll = table.verticalScrollBar().value()
            table.setUpdatesEnabled(False)
            try:
                model.update(snapshot)
                if selected:
                    for index, process in enumerate(model.rows):
                        if (process.pid, process.created) == (selected.pid, selected.created):
                            target = proxy.mapFromSource(model.index(index, 0))
                            if target.isValid():
                                table.selectRow(target.row())
                            break
                table.verticalScrollBar().setValue(scroll)
                button.setEnabled(self._selected(table) is not None)
            finally:
                table.setUpdatesEnabled(True)
        self.summary.setText(f"{len(snapshot.processes)} Prozesse   |   CPU: {decimal(snapshot.cpu, 0)} %   |   "
                             f"Arbeitsspeicher: {decimal(snapshot.memory_percent, 0)} %")
        self.summary.setToolTip(f"{snapshot.skipped} Prozesse wegen fehlender Rechte oder Prozessende nicht lesbar.")
        self._update_performance(snapshot)
        days, seconds = divmod(int(snapshot.uptime), 86400)
        hours, seconds = divmod(seconds, 3600)
        minutes = seconds // 60
        uptime = f"{days} T. {hours:02d}:{minutes:02d} Std."
        self.system_live.setText(f"Systemlaufzeit: {uptime}    ·    {len(snapshot.processes)} lesbare Prozesse")

    def _update_performance(self, s):
        self.resources[0].push(s.cpu, f"{decimal(s.cpu, 0)} %")
        self.resources[1].push(s.memory_percent, f"{size(s.memory_used)} · {decimal(s.memory_percent, 0)} %")
        self.resources[2].push(s.disk_read + s.disk_write, f"{size(s.disk_read + s.disk_write)}/s", 1024**2)
        self.resources[3].push(s.network_recv + s.network_sent, f"{size(s.network_recv + s.network_sent)}/s", 1024**2)
        self.metrics[0].setText(f"{decimal(s.cpu, 0)} %")
        self.metrics[1].setText(f"{decimal(s.memory_percent, 0)} %")
        self.metrics[2].setText(f"{size(s.disk_read + s.disk_write)}/s")
        self.metrics[3].setText(f"{size(s.network_recv + s.network_sent)}/s")
        self.charts[0].push(s.cores if self.per_core.isChecked() else [s.cpu])
        self.charts[1].push([s.memory_percent])
        self.charts[2].push([s.disk_read, s.disk_write], 1024**2)
        self.charts[3].push([s.network_recv, s.network_sent], 1024**2)
        for index in (2, 3):
            self.scales[index].setText(f"{size(self.charts[index].maximum)}/s")
        self.stats[0].setText(f"Auslastung       {decimal(s.cpu)} %\n"
                              f"Logische Prozessoren       {len(s.cores)}\n"
                              f"Prozesse       {len(s.processes)}    ·    Threads       {sum(p.threads for p in s.processes)}")
        self.stats[1].setText(f"In Verwendung       {size(s.memory_used)}    ·    Verfügbar       {size(s.memory_available)}\n"
                              f"Gesamt       {size(s.memory_total)}\n"
                              f"Swap       {size(s.swap_used)} / {size(s.swap_total)}")
        self.stats[2].setText(f"Lesen       {size(s.disk_read)}/s\nSchreiben       {size(s.disk_write)}/s\n"
                              "Summierte E/A-Zähler; virtuelle Geräte können Daten mehrfach zählen.")
        self.stats[3].setText(f"Empfangen       {size(s.network_recv)}/s\nSenden       {size(s.network_sent)}/s\n"
                              "Summierte Schnittstellen; VPNs und Bridges können Daten mehrfach zählen.")

    def _context(self, table, point):
        index = table.indexAt(point)
        if not index.isValid():
            return
        table.selectRow(index.row())
        row = self._selected(table)
        if row is None:
            return
        menu = QMenu(self)
        menu.addAction("Task beenden", lambda: self._end(table))
        menu.addAction("Beenden erzwingen …", lambda: self._end(table, force=True))
        menu.addSeparator()
        menu.addAction("Befehlszeile kopieren", lambda: QApplication.clipboard().setText(row.command or row.name))
        if table is self.tables[0]:
            menu.addAction("Zu Details wechseln", lambda: self._go_details(row))
        menu.exec(table.viewport().mapToGlobal(point))

    def _go_details(self, row):
        self.searches[1].clear()
        self.mine_checks[1].setChecked(False)
        self.tabs.setCurrentIndex(2)
        for index, candidate in enumerate(self.models[1].rows):
            if (candidate.pid, candidate.created) == (row.pid, row.created):
                target = self.filters[1].mapFromSource(self.models[1].index(index, 0))
                self.tables[1].selectRow(target.row())
                self.tables[1].scrollTo(target)
                return

    def _end(self, table, force=False):
        row = self._selected(table)
        if row is None:
            return
        if row.pid in (0, 1, os.getpid()):
            QMessageBox.information(self, "Geschützter Prozess", "Dieser Prozess kann hier nicht beendet werden.")
            return
        action = "Beenden erzwingen" if force else "Task beenden"
        text = f"„{row.name}“ (PID {row.pid}) beenden?\n\nNicht gespeicherte Daten können verloren gehen."
        if force:
            text += "\nDer Prozess erhält keine Möglichkeit, Daten zu speichern (SIGKILL)."
        else:
            text += "\nDie App fordert den Prozess zum Beenden auf (SIGTERM)."
        if QMessageBox.warning(self, action, text, QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
                               QMessageBox.StandardButton.No) != QMessageBox.StandardButton.Yes:
            return
        try:
            end_process(row.pid, row.created, force)
        except psutil.NoSuchProcess:
            QMessageBox.information(self, "Prozess beendet", "Der Prozess wurde bereits beendet.")
        except psutil.AccessDenied:
            QMessageBox.warning(self, "Keine Berechtigung", "Du darfst diesen Prozess nicht beenden.\n"
                                "Die App läuft mit den Rechten deines Benutzers.")
        except (ValueError, OSError) as error:
            QMessageBox.warning(self, "Task konnte nicht beendet werden", str(error))
        else:
            self.statusBar().showMessage(f"{'SIGKILL' if force else 'SIGTERM'} an {row.name} gesendet.", 5000)
        self._refresh()

    def _new_task(self):
        command, accepted = QInputDialog.getText(self, "Neuen Task ausführen", "Programm und Argumente:")
        if not accepted or not command.strip():
            return
        try:
            # Explicit argument parsing: never execute arbitrary text via a shell.
            arguments = shlex.split(command)
            if not arguments or not arguments[0]:
                raise ValueError("Bitte gib einen Programmnamen ein.")
            started, _ = QProcess.startDetached(arguments[0], arguments[1:])
            if not started:
                raise OSError(f"„{arguments[0]}“ konnte nicht gestartet werden.")
        except (ValueError, OSError) as error:
            QMessageBox.warning(self, "Start fehlgeschlagen", str(error))

    def _top(self, checked):
        self.setWindowFlag(Qt.WindowType.WindowStaysOnTopHint, checked)
        self.show()

    def _pause(self, paused):
        self.worker.paused = paused
        self._live_label()
        if not paused:
            self.worker.wake.set()

    def _interval(self, seconds):
        self.worker.interval = seconds
        self.worker.wake.set()
        self._live_label()

    def _live_label(self):
        self.live.setText("Ⅱ  Pausiert" if self.worker.paused else f"●  Live · {decimal(self.worker.interval)} s")
        self.live.setStyleSheet(f"color: {'#99732c' if self.worker.paused else '#207957'};")

    def _refresh(self):
        if self.worker.paused:
            self.pause_action.setChecked(False)
        self.worker.wake.set()

    def _error(self, message):
        self.live.setText("Messung fehlgeschlagen")
        self.live.setStyleSheet("color: #b44735;")
        self.live.setToolTip(message)

    def _tab_changed(self, index):
        if index in (0, 2):
            self.searches[0 if index == 0 else 1].setFocus()

    def _about(self):
        QMessageBox.about(self, "Über Task-Manager",
            f"<h3>Task-Manager {__version__}</h3>"
            "<p>Ein nativer Linux-Systemmonitor mit einer Oberfläche im Stil von Windows 10.</p>"
            "<p>Python · Qt / PySide6 · psutil<br>Lizenz: MIT (App-Code)</p>"
            "<p>CPU-Werte der Prozesse sind über alle logischen Kerne normalisiert. "
            "Die erste Messung benötigt einen Aktualisierungszyklus. "
            "Nicht lesbare E/A-Werte erscheinen als —.</p>")

    def closeEvent(self, event):
        self.worker.stop()
        if self.settings:
            self.settings.setValue("geometry", self.saveGeometry())
            self.settings.setValue("always_on_top", self.top_action.isChecked())
        super().closeEvent(event)


def main(argv=None):
    parser = argparse.ArgumentParser(description="Nativer Linux-Task-Manager")
    parser.add_argument("--version", action="version", version=f"Task-Manager {__version__}")
    parser.add_argument("--install", action="store_true", help="Grafische Installation für diesen Benutzer")
    parser.add_argument("--uninstall", action="store_true", help="Grafische Deinstallation")
    parser.add_argument("--smoke-test", action="store_true", help="GUI und Live-Messung prüfen, dann beenden")
    parser.add_argument("--screenshot", type=Path, help="Beim GUI-Test ein Bildschirmfoto speichern")
    args = parser.parse_args(argv)
    if sys.platform != "linux":
        parser.error("Diese Version ist für Linux vorgesehen.")
    app = QApplication([sys.argv[0]])
    app.setApplicationName(APP_ID)
    app.setApplicationDisplayName("Task-Manager")
    app.setDesktopFileName(APP_ID)
    app.setWindowIcon(QIcon(str(ICON_PATH)))
    app.setStyle("Fusion")
    check_icon = ICON_PATH.with_name("check.svg").as_posix()
    app.setStyleSheet(STYLE + f'QCheckBox::indicator:checked {{ image: url("{check_icon}"); }}')
    window = TaskWindow(settings_enabled=not args.smoke_test)
    window.show()
    if args.install:
        QTimer.singleShot(100, lambda: install_dialog(window, ICON_PATH))
    if args.uninstall:
        QTimer.singleShot(100, lambda: uninstall_dialog(window))
    if args.smoke_test:
        def check():
            try:
                if window.samples < 2 or not window.snapshot or not window.snapshot.processes:
                    raise RuntimeError("Keine gültigen Live-Messungen empfangen.")
                if args.screenshot:
                    args.screenshot.parent.mkdir(parents=True, exist_ok=True)
                    if not window.grab().save(str(args.screenshot)):
                        raise RuntimeError("Bildschirmfoto konnte nicht gespeichert werden.")
                    window.tabs.setCurrentIndex(1)
                    app.processEvents()
                    window.grab().save(str(args.screenshot.with_stem(args.screenshot.stem + "-leistung")))
                print(f"GUI OK: {window.samples} Messungen, {len(window.snapshot.processes)} Prozesse", flush=True)
                window.close()
                app.exit(0)
            except Exception as error:
                print(f"GUI-Test fehlgeschlagen: {error}", file=sys.stderr, flush=True)
                window.close()
                app.exit(1)
        QTimer.singleShot(2800, check)
    return app.exec()
