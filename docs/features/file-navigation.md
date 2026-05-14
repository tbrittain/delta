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

`Tab` toggles focus between the file list and the diff view. The focused panel has a blue border; the unfocused panel has a dark gray border.

---

## Notes panel

Pressing `Tab` from the diff view opens the Notes panel when there are notes. It replaces the diff view in the right panel and shows all notes left during the session.

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate between notes |
| `Enter` | Jump directly to the note's file and hunk in the diff view |
| `Space` | Expand / collapse the full note text |
| `e` | Jump to the note's hunk and enter edit mode |
| `d` | Delete the selected note |
| `Tab` | Return to the diff view |

The panel only appears in the Tab cycle when at least one note exists.

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
