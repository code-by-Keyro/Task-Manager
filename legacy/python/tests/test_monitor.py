import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

import psutil
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication

from task_manager.install import install_files
from task_manager.model import ProcessModel, ProcessFilter, SORT_ROLE
from task_manager.monitor import Sampler, end_process


class MonitorTests(unittest.TestCase):
    def test_real_snapshot(self):
        sampler = Sampler()
        sampler.sample()
        time.sleep(0.1)
        snapshot = sampler.sample()
        self.assertGreater(snapshot.memory_total, 0)
        self.assertGreater(len(snapshot.cores), 0)
        self.assertGreaterEqual(snapshot.cpu, 0)
        self.assertLessEqual(snapshot.cpu, 100)
        self.assertIn(os.getpid(), [p.pid for p in snapshot.processes])
        self.assertGreaterEqual(snapshot.network_recv, 0)
        self.assertGreaterEqual(snapshot.disk_write, 0)

    def test_only_signal_child_and_reject_stale_identity(self):
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        try:
            created = psutil.Process(child.pid).create_time()
            with self.assertRaises(ValueError):
                end_process(child.pid, created + 1)
            self.assertIsNone(child.poll())
            end_process(child.pid, created)
            child.wait(timeout=5)
            self.assertLess(child.returncode, 0)
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()

    def test_protected_processes(self):
        for pid in (0, 1, os.getpid()):
            with self.assertRaises(ValueError):
                end_process(pid, 0)


class DesktopTests(unittest.TestCase):
    def test_install_in_path_with_spaces_and_keep_unrelated_launcher(self):
        with tempfile.TemporaryDirectory(prefix="task manager ") as folder:
            base = Path(folder)
            source = base / "source"
            source.write_bytes(b"test binary")
            icon = base / "icon.svg"
            icon.write_text("<svg/>")
            binary = install_files(source, icon, base / "data", base / "bin")
            self.assertEqual(binary.read_bytes(), source.read_bytes())
            self.assertTrue(os.access(binary, os.X_OK))
            self.assertEqual((base / "bin/task-manager").resolve(), binary)
            desktop = next((base / "data/applications").glob("*.desktop"))
            self.assertIn("Terminal=false", desktop.read_text())
            self.assertIn(f'Exec="{binary}"', desktop.read_text())
            # Reinstalling from the installed binary must also work.
            install_files(binary, icon, base / "data", base / "bin")
            if shutil_available("desktop-file-validate"):
                subprocess.run(["desktop-file-validate", str(desktop)], check=True)
            (base / "bin/task-manager").unlink()
            (base / "bin/task-manager").write_text("unrelated")
            with self.assertRaises(FileExistsError):
                install_files(source, icon, base / "data", base / "bin")
            self.assertEqual((base / "bin/task-manager").read_text(), "unrelated")


def shutil_available(command):
    import shutil
    return shutil.which(command)


class ModelTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_search_and_numeric_sort(self):
        snapshot = Sampler().sample()
        model = ProcessModel()
        model.update(snapshot)
        proxy = ProcessFilter()
        proxy.setSourceModel(model)
        proxy.sort(3, Qt.SortOrder.DescendingOrder)
        memory = [proxy.index(i, 3).data(SORT_ROLE) for i in range(proxy.rowCount())]
        self.assertEqual(memory, sorted(memory, reverse=True))
        proxy.set_query(str(os.getpid()))
        self.assertGreater(proxy.rowCount(), 0)
        proxy.set_query("no-process-can-have-this-name-91e643a9")
        self.assertEqual(proxy.rowCount(), 0)


if __name__ == "__main__":
    unittest.main()
