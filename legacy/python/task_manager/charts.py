from collections import deque

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import QColor, QPainter, QPainterPath, QPen
from PySide6.QtWidgets import QWidget, QPushButton


class Chart(QWidget):
    def __init__(self, color="#1485c5", parent=None):
        super().__init__(parent)
        self.color = QColor(color)
        self.series = [deque(maxlen=60)]
        self.maximum = 100.0
        self.setMinimumSize(200, 190)
        self.setAccessibleName("Verlauf der Systemauslastung")

    def push(self, values, maximum=100.0):
        if len(self.series) != len(values):
            self.series = [deque(maxlen=60) for _ in values]
        for series, value in zip(self.series, values):
            series.append(value)
        self.maximum = max(1.0, maximum, max((max(s, default=0) for s in self.series), default=0))
        self.update()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        rectangle = QRectF(1, 1, self.width() - 2, self.height() - 2)
        painter.fillRect(rectangle, QColor("#fcfeff"))
        painter.setPen(QPen(QColor("#d9eaf3"), 1))
        for x in range(1, 12):
            position = rectangle.left() + rectangle.width() * x / 12
            painter.drawLine(QPointF(position, rectangle.top()), QPointF(position, rectangle.bottom()))
        for y in range(1, 8):
            position = rectangle.top() + rectangle.height() * y / 8
            painter.drawLine(QPointF(rectangle.left(), position), QPointF(rectangle.right(), position))
        for number, series in enumerate(self.series):
            if not series:
                continue
            path = QPainterPath()
            points = [QPointF(rectangle.right() - (len(series) - 1 - i) * rectangle.width() / 59,
                              rectangle.bottom() - min(value / self.maximum, 1) * rectangle.height())
                      for i, value in enumerate(series)]
            path.moveTo(points[0])
            for point in points[1:]:
                path.lineTo(point)
            color = self.color if number == 0 else QColor("#a965c8")
            if len(self.series) > 2:
                color = QColor.fromHsv((number * 41 + 195) % 360, 170, 180)
            if len(self.series) == 1:
                fill = QPainterPath(path)
                fill.lineTo(points[-1].x(), rectangle.bottom())
                fill.lineTo(points[0].x(), rectangle.bottom())
                fill.closeSubpath()
                background = QColor(color)
                background.setAlpha(35)
                painter.fillPath(fill, background)
            painter.setPen(QPen(color, 1.7))
            painter.drawPath(path)
        painter.setPen(QPen(self.color, 1))
        painter.drawRect(rectangle)


class ResourceButton(QPushButton):
    def __init__(self, title, color, parent=None):
        super().__init__(parent)
        self.title = title
        self.detail = "Wird gemessen …"
        self.color = QColor(color)
        self.history = deque([0.0] * 30, maxlen=30)
        self.maximum = 100.0
        self.setCheckable(True)
        self.setFixedHeight(86)
        self.setMinimumWidth(220)
        self.setAccessibleName(title)
        self.setCursor(Qt.CursorShape.PointingHandCursor)

    def push(self, value, detail, maximum=100.0):
        self.history.append(value)
        self.detail = detail
        self.maximum = max(1.0, maximum, max(self.history))
        self.setAccessibleDescription(detail)
        self.update()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        background = "#e7f2fa" if self.isChecked() else "#f4f8fb" if self.underMouse() else "#ffffff"
        painter.fillRect(self.rect(), QColor(background))
        if self.isChecked():
            painter.fillRect(0, 0, 3, self.height(), QColor("#0078d7"))
        rect = QRectF(15, 20, 62, 45)
        painter.fillRect(rect, QColor("#f8fcff"))
        path = QPainterPath(QPointF(rect.left(), rect.bottom()))
        for i, value in enumerate(self.history):
            path.lineTo(rect.left() + i * rect.width() / 29,
                        rect.bottom() - min(value / self.maximum, 1) * rect.height())
        painter.setPen(QPen(self.color, 1.5))
        painter.drawPath(path)
        painter.drawRect(rect)
        font = painter.font()
        font.setPointSize(11)
        painter.setFont(font)
        painter.setPen(QColor("#202832"))
        painter.drawText(QRectF(91, 19, self.width() - 96, 24), self.title)
        font.setPointSize(9)
        painter.setFont(font)
        painter.setPen(QColor("#536574"))
        painter.drawText(QRectF(91, 44, self.width() - 96, 24), self.detail)
        if self.hasFocus():
            painter.setPen(QPen(QColor("#0078d7"), 1, Qt.PenStyle.DotLine))
            painter.drawRect(self.rect().adjusted(4, 4, -4, -4))
