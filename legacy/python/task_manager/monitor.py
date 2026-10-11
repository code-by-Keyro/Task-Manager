"""Read-only system sampling; no GUI dependencies and no shell commands."""
from dataclasses import dataclass, field
import os
import platform
import time

import psutil


@dataclass(frozen=True)
class ProcessRow:
    pid: int
    created: float
    name: str
    status: str
    cpu: float
    memory: int
    io_rate: float | None
    user: str
    threads: int
    command: str


@dataclass(frozen=True)
class Snapshot:
    timestamp: float
    cpu: float
    cores: list[float]
    memory_percent: float
    memory_used: int
    memory_total: int
    memory_available: int
    swap_used: int
    swap_total: int
    disk_read: float
    disk_write: float
    network_recv: float
    network_sent: float
    uptime: float
    processes: list[ProcessRow] = field(default_factory=list)
    skipped: int = 0


def cpu_name() -> str:
    try:
        with open("/proc/cpuinfo", encoding="utf-8") as file:
            for line in file:
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or platform.machine()


def distribution() -> str:
    try:
        return platform.freedesktop_os_release().get("PRETTY_NAME", "Linux")
    except OSError:
        return "Linux"


class Sampler:
    def __init__(self):
        self.cpu_count = psutil.cpu_count() or 1
        self.previous_time = time.monotonic()
        self.previous_disk = psutil.disk_io_counters()
        self.previous_net = self._network()
        self.previous_io: dict[tuple[int, float], int] = {}
        psutil.cpu_percent()
        psutil.cpu_percent(percpu=True)

    @staticmethod
    def _network() -> tuple[int, int]:
        interfaces = psutil.net_io_counters(pernic=True)
        return (sum(v.bytes_recv for k, v in interfaces.items() if k != "lo"),
                sum(v.bytes_sent for k, v in interfaces.items() if k != "lo"))

    def sample(self) -> Snapshot:
        now = time.monotonic()
        elapsed = max(now - self.previous_time, 0.001)
        memory = psutil.virtual_memory()
        swap = psutil.swap_memory()
        disk = psutil.disk_io_counters()
        network = self._network()
        read = write = 0.0
        if disk is not None and self.previous_disk is not None:
            read = max(0, disk.read_bytes - self.previous_disk.read_bytes) / elapsed
            write = max(0, disk.write_bytes - self.previous_disk.write_bytes) / elapsed
        processes = []
        next_io = {}
        skipped = 0
        # process_iter caches Process instances, preserving cpu_percent baselines.
        for process in psutil.process_iter():
            try:
                with process.oneshot():
                    created = process.create_time()
                    key = (process.pid, created)
                    io_rate = None
                    try:
                        io = process.io_counters()
                        total_io = io.read_bytes + io.write_bytes
                        next_io[key] = total_io
                        if key in self.previous_io:
                            io_rate = max(0, total_io - self.previous_io[key]) / elapsed
                    except (psutil.AccessDenied, psutil.ZombieProcess, AttributeError):
                        pass
                    try:
                        command = " ".join(process.cmdline())
                    except (psutil.AccessDenied, psutil.ZombieProcess):
                        command = ""
                    try:
                        user = process.username()
                    except (psutil.AccessDenied, psutil.ZombieProcess):
                        user = "—"
                    processes.append(ProcessRow(
                        process.pid, created, process.name(), process.status(),
                        min(100.0, max(0.0, process.cpu_percent() / self.cpu_count)),
                        process.memory_info().rss, io_rate, user,
                        process.num_threads(), command,
                    ))
            except (psutil.NoSuchProcess, psutil.AccessDenied, psutil.ZombieProcess):
                skipped += 1
        snapshot = Snapshot(
            now, psutil.cpu_percent(), psutil.cpu_percent(percpu=True),
            memory.percent, memory.total - memory.available, memory.total,
            memory.available, swap.used, swap.total, read, write,
            max(0, network[0] - self.previous_net[0]) / elapsed,
            max(0, network[1] - self.previous_net[1]) / elapsed,
            max(0, time.time() - psutil.boot_time()), processes, skipped,
        )
        self.previous_time = now
        self.previous_disk = disk
        self.previous_net = network
        self.previous_io = next_io
        return snapshot


def end_process(pid: int, created: float, force: bool = False) -> None:
    """Verify process identity before signaling, so a reused PID is never targeted."""
    if pid in (0, 1, os.getpid()):
        raise ValueError("Dieser Prozess ist geschützt und kann hier nicht beendet werden.")
    process = psutil.Process(pid)
    if process.create_time() != created:
        raise ValueError("Der ursprüngliche Prozess existiert nicht mehr. Die Liste wurde aktualisiert.")
    if force:
        process.kill()
    else:
        process.terminate()
