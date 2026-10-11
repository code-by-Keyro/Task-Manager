# Project guidance

Native Linux desktop task manager using Rust and egui/eframe.
Keep the Windows 10 inspired light interface and existing dark mode.
Use configurable German/English text with English fallback and English reference
keys in config/languages.json. Preserve personal translation configurations.

## Development

Read the relevant Vault notes and existing Git diff before editing. Preserve
uncommitted work. Active source is in src/; legacy/python/ is a historical reference.
Keep blocking I/O and external commands in workers. Preserve bounded channels,
identity checks, confirmation dialogs and honest handling of missing measurements.

Use the installed, runtime-matched ProjectAtlas skill and MCP tools before broad
source reads. Select the exact checkout. Initialize only absent project-local
state; otherwise start with atlas_session_brief and targeted summaries/slices.
Refresh changes incrementally with atlas_watch_once. Follow typed full-refresh
guidance when needed. ProjectAtlas databases remain local and ignored.

After meaningful application changes run cargo fmt --check,
cargo clippy --all-targets -- -D warnings and cargo test.
Use cargo run -- --check for read-only sampling and --smoke-test in a graphical
session for real window checks. Never send process signals during UI smoke tests.
Signal tests use only disposable subprocesses created by the test itself.
Build downloads with bash scripts/build-rust.sh. Preserve licenses and permissions.

## Documentation and publication

Vault/ contains the public project knowledge. Maintain relevant notes, decisions,
tasks and dated journal entries before completing work. Distinguish planned,
implemented and verified results. Keep actual test limits explicit.
Public notes and screenshots must use synthetic or anonymized data. Keep private
machine reports, accounts, credentials and local deployment records outside the
public repository. Check tracked files and history before publication.

Cargo.lock, config/, code, scripts, assets, licenses and public documentation are
versioned. Generated builds, caches and local agent settings remain ignored.
Use the configured repository and normal authorized authentication. Check diffs
before committing and verify each remote commit after an authorized push.
Do not rewrite shared history or force-push without explicit authorization.
