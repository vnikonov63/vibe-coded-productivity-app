# tui-timer

A terminal-based time tracker with persistent state. Timers keep running even when the app is closed.

## Quick start

```bash
cargo run -- -f timer/main_timer.json
```

The `-f` flag points to the JSON file that stores your timers.
The file and its parent directory are created automatically on first run.

You can maintain multiple separate timer files:

```bash
cargo run -- -f timer/work.json
cargo run -- -f timer/personal.json
```

If `-f` is omitted the app defaults to `timer/main_timer.json`.

## Two-level structure

Every timer can have sub-timers (one level deep).

- A parent timer's displayed time = its own elapsed + **sum of all children's elapsed**.
- Starting a child timer automatically increases the parent's total too.
- Navigate into a timer's children with `l` or `→`, back out with `h`, `←`, or `Esc`.

## Keybindings

### Normal mode — top level

| Key          | Action                                  |
| ------------ | --------------------------------------- |
| `i`          | Add a new top-level timer               |
| `Enter`      | Start / stop the selected timer's clock |
| `l` / `→`    | Open the selected timer's children      |
| `j` / `↓`    | Move selection down                     |
| `k` / `↑`    | Move selection up                       |
| `d d d`      | Start delete sequence (3 × `d`)         |
| `q` / `Esc`  | Quit                                    |

### Normal mode — children view _(opened with `l` / `→`)_

| Key               | Action                                  |
| ----------------- | --------------------------------------- |
| `i`               | Add a new sub-timer inside this group   |
| `Enter`           | Start / stop the selected sub-timer     |
| `h` / `←` / `Esc`| Go back to the top-level list           |
| `j` / `↓`         | Move selection down                     |
| `k` / `↑`         | Move selection up                       |
| `d d d`           | Start delete sequence for the sub-timer |

### Insert mode _(opened with `i`)_

| Key         | Action                 |
| ----------- | ---------------------- |
| `Enter`     | Confirm — add timer    |
| `Esc`       | Cancel                 |
| `Backspace` | Delete last character  |

### Delete sequence _(after first `d`)_

| Key           | Action                                        |
| ------------- | --------------------------------------------- |
| `d` (×2 more) | Advance — three `d` presses reach confirmation |
| Any other key | Cancel — return to normal                     |

### Confirm delete _(after `d d d`)_

| Key           | Action              |
| ------------- | ------------------- |
| `y`           | Delete the timer    |
| Any other key | Cancel              |

## Time display

| Elapsed        | Format shown            |
| -------------- | ----------------------- |
| Under 1 minute | `42 seconds`            |
| Under 1 hour   | `5 minutes, 12 seconds` |
| 1 hour or more | `2 hours, 30 minutes`   |

## State file format

Plain JSON — safe to edit by hand:

```json
[
  {
    "name": "Work",
    "total_seconds": 0,
    "running_since": null,
    "children": [
      {
        "name": "Deep focus",
        "total_seconds": 5400,
        "running_since": null
      },
      {
        "name": "Meetings",
        "total_seconds": 1800,
        "running_since": 1711612800
      }
    ]
  }
]
```

`running_since` is a Unix timestamp. If set, the timer is still running and elapsed time is computed live. The parent's total reflects both its own time and its children's time automatically.
