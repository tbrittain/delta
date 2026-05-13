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

## Known issues / open feedback

No significant feedback collected yet.
