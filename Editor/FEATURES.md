# Masm Editor — Feature Roadmap

This document outlines planned features for the Masm Editor (MicroASM-focused Qt editor). Each feature includes a short description, priority, acceptance criteria, and suggested implementation notes.

---

## 1) Symbols resolver improvements (High)
- Description: Enhance the `SymbolsViewer` to parse and present more symbol types: `LBL` directives, label definitions, data labels (DB/DW/DD/DQ/DF/DDbl/RES*), `STATE` blocks, macros, and include targets. Add filtering and search within the panel.
- Acceptance:
  - SymbolsViewer lists new categories and groups items by type.
  - Double-clicking an item navigates to the symbol's source line.
  - A small filter box allows typing to narrow symbols by name or type.
- Files: `SymbolsViewer.{h,cpp}`, `main.cpp`, `CodeEditor.{h,cpp}`
- Notes: Use `QTreeWidget` or `QListView` with a model; store line numbers in `Qt::UserRole`.

## 2) Diagnostics (squiggles) from masm output (High)
- Description: Run `masm` on save/Build, parse its output for file/line/col messages and show squiggles in the editor as well as a Problems pane.
- Acceptance:
  - Squiggles appear under error regions with hover details.
  - Problems pane lists diagnostics and allows navigation on click.
- Files: `Diagnostics.{h,cpp}`, `CodeEditor.cpp`, `main.cpp`.
- Notes: Debounce runs to avoid frequent full builds. Map masm messages to document offsets.

## 3) Hover tooltips & signature help (Medium)
- Description: Show short documentation or opcode signatures on hover; provide signature help while typing operands.
- Acceptance:
  - Hover shows brief description for instructions and directives.
  - Signature helper appears after typing an operand delimiter.
- Files: `MicroHighlighter.cpp`, `MicroDocs.{h,cpp}`.
- Notes: Preprocess `MicroV2.md` and directive docs into a compact JSON or map at build time.

## 4) Go-to-definition & find usages (Medium)
- Description: Index symbols for 'Go to Definition' (F12) and 'Find Usages'.
- Acceptance:
  - F12 on a symbol jumps to its definition.
  - Find Usages lists references and allows navigation.
- Files: `Indexer.{h,cpp}`, `CodeEditor.cpp`, `SymbolsViewer.cpp`.
- Notes: Keep the index incremental for performance.

## 5) Formatter / auto-indent (Medium)
- Description: Implement a formatter and provide auto-indent rules for labels and instructions.
- Acceptance:
  - Document can be formatted via `Format Document` action.
  - Pressing Enter auto-indents to a sensible column for operands.
- Files: `Formatter.{h,cpp}`, `CodeEditor.cpp`.

## 6) Settings UI (MASM path, theme, completions) (Low)
- Description: Settings dialog to configure MASM path, theme (light/dark), autocomplete behavior, and persistence via `QSettings`.
- Acceptance:
  - Settings persist and are respected by build/run and UI.
- Files: `SettingsDialog.{h,cpp}`, `main.cpp`.

## 7) Live linting & background parsing (Low)
- Description: Add a debounced background parser that updates diagnostics and symbols without blocking UI.
- Acceptance:
  - Parser runs off the UI thread and updates UI within ~500ms of edits.
- Files: `ParserWorker.{h,cpp}`, `main.cpp`.

## 8) Snippets & templates (Low)
- Description: Provide snippet expansion for common assembly constructs and integrate with the completer.
- Acceptance:
  - Snippets are available via completion and expand with Tab.
- Files: `Snippets.{h,cpp}`, resources.

## 9) Project explorer (Low)
- Description: Dockable file tree showing workspace folders and files with context actions.
- Acceptance:
  - Can open files, add/remove files, run project build commands.
- Files: `ProjectExplorer.{h,cpp}`, `main.cpp`.

## 10) Tests & CI (High)
- Description: Add unit tests for parser/highlighter and a CI job to build and run tests.
- Acceptance:
  - Tests run locally and in CI, protecting regressing changes to critical parsing logic.
- Files: `tests/*`, `CMakeLists.txt` updates.

---

### Next steps (short-term)
1. Implement Symbols resolver improvements (filtering/search, more symbol types).
2. Add diagnostics plumbing to parse masm output and show squiggles.
3. Implement hover tooltips using existing docs.

If you'd like, I can start implementing item 1 immediately. Which feature should I pick next?