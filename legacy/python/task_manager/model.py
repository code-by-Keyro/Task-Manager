import getpass

from PySide6.QtCore import QAbstractTableModel, QModelIndex, Qt, QSortFilterProxyModel
from PySide6.QtGui import QColor
from PySide6.QtWidgets import QStyledItemDelegate, QStyle

from .monitor import ProcessRow

SORT_ROLE = int(Qt.ItemDataRole.UserRole) + 1
ROW_ROLE = int(Qt.ItemDataRole.UserRole) + 2
STATUS = {
    "running": "Wird ausgeführt", "sleeping": "Bereit", "disk-sleep": "Wartet auf E/A",
    "stopped": "Angehalten", "tracing-stop": "Angehalten", "zombie": "Zombie",
    "dead": "Beendet", "idle": "Inaktiv", "waking": "Wird aktiviert",
    "parked": "Geparkt", "locked": "Gesperrt", "waiting": "Wartet",
}


def decimal(value: float, digits: int = 1) -> str:
    return f"{value:.{digits}f}".replace(".", ",")


def size(value: float) -> str:
    for suffix in ("B", "KiB", "MiB", "GiB", "TiB"):
        if abs(value) < 1024 or suffix == "TiB":
            return f"{decimal(value)} {suffix}"
        value /= 1024
    return "—"


class ProcessModel(QAbstractTableModel):
    def __init__(self, detailed=False, parent=None):
        super().__init__(parent)
        self.rows: list[ProcessRow] = []
        self.detailed = detailed
        self.cpu = self.memory = 0.0
        self.columns = (["Name", "PID", "Benutzer", "Status", "CPU", "Arbeitsspeicher", "Threads", "Befehlszeile"]
                        if detailed else ["Name", "Status", "CPU", "Arbeitsspeicher", "Datenträger", "PID"])

    def update(self, snapshot):
        self.beginResetModel()
        self.rows = snapshot.processes
        self.cpu, self.memory = snapshot.cpu, snapshot.memory_percent
        self.endResetModel()
        self.headerDataChanged.emit(Qt.Orientation.Horizontal, 0, len(self.columns) - 1)

    def rowCount(self, parent=QModelIndex()):
        return 0 if parent.isValid() else len(self.rows)

    def columnCount(self, parent=QModelIndex()):
        return len(self.columns)

    def headerData(self, section, orientation, role=Qt.ItemDataRole.DisplayRole):
        if orientation != Qt.Orientation.Horizontal or not 0 <= section < len(self.columns):
            return None
        column = self.columns[section]
        if role == Qt.ItemDataRole.DisplayRole:
            if not self.detailed and column in ("CPU", "Arbeitsspeicher"):
                percent = self.cpu if column == "CPU" else self.memory
                return f"{decimal(percent, 0)} %\n{column}"
            return column
        if role == Qt.ItemDataRole.ToolTipRole:
            return {"CPU": "Anteil an der gesamten CPU-Kapazität, über alle logischen Kerne normalisiert.",
                    "Arbeitsspeicher": "Residenter Speicher (RSS); gemeinsam genutzte Seiten können mehrfach vorkommen.",
                    "Datenträger": "Lese- und Schreibrate dieses Prozesses. — bedeutet: keine Messung verfügbar."}.get(column)
        if role == Qt.ItemDataRole.TextAlignmentRole:
            return int(Qt.AlignmentFlag.AlignVCenter | Qt.AlignmentFlag.AlignLeft)

    def data(self, index, role=Qt.ItemDataRole.DisplayRole):
        if not index.isValid() or index.row() >= len(self.rows):
            return None
        row = self.rows[index.row()]
        column = self.columns[index.column()]
        values = {"Name": row.name, "Status": STATUS.get(row.status, row.status), "CPU": row.cpu,
                  "Arbeitsspeicher": row.memory, "Datenträger": row.io_rate,
                  "PID": row.pid, "Benutzer": row.user, "Threads": row.threads, "Befehlszeile": row.command}
        value = values[column]
        if role == ROW_ROLE:
            return row
        if role == SORT_ROLE:
            return -1 if value is None else value
        if role == Qt.ItemDataRole.DisplayRole:
            if column == "CPU":
                return f"{decimal(value)} %"
            if column == "Arbeitsspeicher":
                return f"{decimal(value / 1024**2)} MiB"
            if column == "Datenträger":
                return "—" if value is None else f"{decimal(value / 1024**2)} MiB/s"
            return str(value)
        if role == Qt.ItemDataRole.TextAlignmentRole:
            return int(Qt.AlignmentFlag.AlignVCenter | (Qt.AlignmentFlag.AlignRight if column in
                       ("CPU", "Arbeitsspeicher", "Datenträger", "PID", "Threads") else Qt.AlignmentFlag.AlignLeft))
        if role == Qt.ItemDataRole.BackgroundRole and column in ("CPU", "Arbeitsspeicher", "Datenträger"):
            strength = (row.cpu / 100 if column == "CPU" else min(row.memory / 1024**3, 1)
                        if column == "Arbeitsspeicher" else min((row.io_rate or 0) / 1024**2 / 10, 1))
            return QColor(255, 247 - int(35 * strength), 214 - int(100 * strength))
        if role == Qt.ItemDataRole.ToolTipRole:
            return row.command or row.name
        return None


class ProcessFilter(QSortFilterProxyModel):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.query = ""
        self.only_mine = False
        self.user = getpass.getuser()
        self.setSortRole(SORT_ROLE)
        self.setSortCaseSensitivity(Qt.CaseSensitivity.CaseInsensitive)

    def set_query(self, text):
        self.beginFilterChange()
        self.query = text.casefold().strip()
        self.endFilterChange(QSortFilterProxyModel.Direction.Rows)

    def set_only_mine(self, checked):
        self.beginFilterChange()
        self.only_mine = checked
        self.endFilterChange(QSortFilterProxyModel.Direction.Rows)

    def filterAcceptsRow(self, row, parent):
        process = self.sourceModel().rows[row]
        if self.only_mine and process.user != self.user:
            return False
        return not self.query or self.query in f"{process.name} {process.pid} {process.user} {process.command}".casefold()


class ResourceDelegate(QStyledItemDelegate):
    """Keep the Windows-style resource heatmap visible with Qt stylesheets."""
    def paint(self, painter, option, index):
        painter.save()
        selected = bool(option.state & QStyle.StateFlag.State_Selected)
        painter.fillRect(option.rect, QColor("#dceefc") if selected else index.data(Qt.ItemDataRole.BackgroundRole))
        painter.setPen(QColor("#182f43" if selected else "#202832"))
        painter.setFont(option.font)
        rect = option.rect.adjusted(10, 0, -10, -1)
        text = option.fontMetrics.elidedText(str(index.data()), Qt.TextElideMode.ElideRight, rect.width())
        painter.drawText(rect, int(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter), text)
        painter.setPen(QColor("#eee8d3"))
        painter.drawLine(option.rect.bottomLeft(), option.rect.bottomRight())
        painter.restore()
