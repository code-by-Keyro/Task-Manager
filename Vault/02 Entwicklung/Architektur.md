# Architektur

src/main.rs startet CLI oder GUI; src/ui.rs, apps_ui.rs und drivers_ui.rs bilden
die Oberfläche. monitor.rs, cpu.rs, hardware.rs und storage.rs erfassen Linux-
Messwerte. windows.rs und processes.rs ordnen Fenster und Prozessgruppen zu.
apps.rs und drivers.rs lesen Inventare; docker.rs, optimizer.rs und packages.rs
verwalten separate Analysen/Aktionen. command.rs begrenzt externe CLI-Abfragen.
install.rs führt die Benutzerinstallation aus. i18n.rs und config/ enthalten
Sprachlogik und Kataloge. Hintergrund-Worker halten die Oberfläche bedienbar;
Bestätigungen und frische Identitätsprüfungen sichern mutierende Aktionen ab.
