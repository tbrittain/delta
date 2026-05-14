# File Navigation

## How it works

The left panel shows all files changed in the diff range. Each entry shows the status indicator and file path.

Files are color-coded by status:

| Indicator | Status | Color |
|---|---|---|
| `[A]` | Added | Green |
| `[M]` | Modified | Yellow |
| `[D]` | Deleted | Red |
| `[R]` | Renamed | Cyan |

A `●` dot appears after the filename when the file has at least one note attached.

The panel title shows the total file count: ` Files (5) `.

### Navigation

| Key | Action |
|---|---|
| `↑` / `↓` | Move selection up/down |
| `Enter` | Open the selected file's diff and switch to the diff panel |
| `Tab` | Switch focus to the diff panel |

When you move the selection, the diff for the newly selected file is loaded automatically in the background. Switching focus to the diff panel is not required to trigger loading.

### Panel switching

`Tab` / `Shift+Tab` cycle focus forward/backward through the panels (file list → diff → notes → file list). The focused panel has a double cyan border; unfocused panels have a dim gray border.

---

## Notes panel

When at least one note exists, a **notes panel** appears below the diff view (10 rows). It is always visible alongside the diff — it does not replace it. Each entry shows the file path and hunk header on the first line, and the first line of the note text (truncated with `…` if needed) on the second line.

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate between notes |
| `Enter` | Jump directly to the note's file and hunk in the diff view |
| `Space` | Expand / collapse the full note text |
| `e` | Jump to the note's hunk and enter edit mode |
| `d` | Delete the selected note |
| `Tab` / `Shift+Tab` | Cycle focus to adjacent panel |

The panel only appears in the Tab cycle when at least one note exists. Deleting the last note automatically removes the panel and returns focus to the diff view.

---

## Planned improvements

### Directory tree view
**Goal:** Replace the flat file list with a proper tree view that mirrors the directory structure of the diff. Directories containing changed files can be expanded or collapsed with a count (e.g. `src/ (3 files)`).

**Notes:**
- The data model already carries full paths — grouping by directory prefix is the main structural change.
- Arrow keys and `Enter` should work on both directory nodes and file leaves.
- The `●` note marker propagates up to directory nodes when any child file has notes.

### Find in files (Ctrl+F, file list focused)
**Goal:** Filter the file list to files whose paths match a typed string. The list narrows as you type. `Esc` clears and restores the full list.

**Notes:** Substring or glob match against the file path. Case-insensitive by default. Search input shown in the status bar.

---

## Known issues / open feedback

### No mouse support

All navigation is keyboard-only. Mouse clicks, scroll wheel, and text selection are not supported.

**Possible directions:**
- Click to select a file in the file list
- Click a note in the Notes panel to jump to it
- Scroll wheel for the diff view
- Click-to-position cursor in comment input

**Priority:** Post-MVP. Would improve accessibility and feel more natural for users less comfortable with keyboard-only TUIs.

---

### No version indicator

The currently installed version is not shown anywhere in the TUI. Users must run `delta --version` or check `Cargo.toml` to find it.

**Possible directions:**
- Show `v0.x.y` in the status bar or a panel title corner

**Priority:** Low. Minor convenience; easy to add.

---

### File list clips long paths

File paths longer than the file list panel width (32 chars) are clipped with no way to see the full path.

**Possible directions:**
- Horizontal scroll in the file list (`←`/`→` when file list is focused)

**Priority:** Low. Only affects repos with deeply nested paths.

---

## In-app help

No `?` keybind or help view exists. Users must consult the README or feature docs externally.

**Possible directions:**
- `?` opens an in-app help overlay listing all keybindings per panel (simplest)
- Embed the feature markdown docs into the binary at compile time (via `include_str!`) and render them in a scrollable view
- GitHub Pages site that renders the `docs/` directory — link surfaced via `delta --help` or the in-app help overlay

**Priority:** Post-MVP. The status bar covers the essentials; full docs are for onboarding new users.
