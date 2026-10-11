# Add a language — Task-Manager 0.12.1

The application detects your Linux session language at startup. German and
English are included. Unsupported languages and missing translations fall back
to English. Keep `"language": "auto"` for automatic selection, or use a language
code to select one explicitly. Restart the application after editing the file.

Your personal configuration is `~/.config/linux-task-manager/languages.json`
(or `$XDG_CONFIG_HOME/linux-task-manager/languages.json` when configured).
The application creates it on its first normal start and preserves existing
files. No rebuild or installation is required to add a language.

The **English block comes first as the reference**. Copy it, change the language
code and translate the values on the right. Keep the English keys on the left.
For example, English uses `"File": "File"`, German uses `"File": "Datei"`.
This minimal example adds French; all other messages still appear in English:

```json
{
  "language": "auto",
  "languages": {
    "fr": {
      "decimal_separator": ",",
      "messages": {
        "File": "Fichier",
        "View": "Affichage",
        "Help": "Aide",
        "Processes": "Processus",
        "Applications ({0})": "Applications ({0})"
      }
    }
  }
}
```

Use lowercase codes such as `fr`, `es` or `pt_br`. German and English remain
available even when omitted from your file. The complete reference is
`config/languages.json` in the source archive.

Keep numbered placeholders such as `{0}` and `{1}`; you can reorder or repeat
them. Invalid placeholders, empty translations and unknown keys produce a
warning and retain the built-in translation or English fallback. Use `\n` for
line breaks. JSON does not allow comments or trailing commas. The decimal
separator may be `.` or `,`; omitted separators default to `.` for new languages.
`Running (container)` is a context key displayed as “Running” in English;
`Running` is the separate process status. `Kernel module` displays “Module”
and stays distinct from the plural key `Modules`.

Legacy German keys from 0.12.0 remain readable. If both old and new keys are
present, the explicit English key wins. Existing files are not rewritten by the
application. A malformed existing file is preserved, and built-in translations
allow the application to start.

Locale precedence is `LC_ALL`, `LC_MESSAGES`, then `LANG`. Outside C/POSIX,
`LANGUAGE` selects the first supported language in its preference list.
C/POSIX or no locale selects English. `task-manager --language-check` reports the
selected language and file path without creating or modifying a configuration.
