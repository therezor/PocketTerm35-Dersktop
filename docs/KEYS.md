# Key map

Two layers of remapping, with one owner each:

* **keyd** (`/etc/keyd/pocketterm35.conf`) turns the PocketTerm35's physical
  keys into *standard* keysyms — a Super key, a working Fn row, arrows.
* **sway** (`/usr/share/pt35-desktop/sway/config`) binds those keysyms to shell
  actions. No shell behaviour is ever bound in keyd; that way nothing is
  remapped twice.

## Shell (sway)

| keys | action |
|---|---|
| `Super`+`Space`, `Menu` | open/close the menu |
| `Super`+`Enter` | terminal |
| `Super`+`q` | close window |
| `Super`+`f` | fullscreen toggle (hides the bar) |
| `Super`+`t` | floating toggle |
| `Super`+`r` | fit an oversized window to the screen |
| `Super`+`p` | arm/disarm the keyboard pointer |
| `Super`+`g` | keyboard pointer, straight into grid jump |
| `Super`+`s` | screenshot to `~` |
| `Super`+`1`…`9` | workspace 1–9 |
| `Super`+`Tab` / `Super`+`Shift`+`Tab` | next / previous workspace |
| `Super`+`PgUp` / `PgDn` | previous / next workspace (shoulder buttons) |
| `Super`+`Shift`+`c` | reload sway |
| `Super`+`Shift`+`e` | power menu |
| volume / brightness keys | `pt35ctl volume`/`brightness` |

## Menu

| keys | action |
|---|---|
| D-pad, arrows, `Tab` | move |
| `Enter`, `Right` | activate |
| `1`–`9` | activate that visible row |
| any letter | filter (digits become literal once a filter is active) |
| `Backspace` | clear the filter, then go up a level |
| `Left` | up a level |
| `Esc` | close |

Destructive entries (`reboot`, `poweroff`) open a Yes/No screen whose cursor
starts on **No**.

## Keyboard pointer

| keys | action |
|---|---|
| D-pad / arrows | move (accelerates while held) |
| `Enter`, `Space` | left click |
| `r` | right click |
| `m` | middle click |
| `PgUp` / `PgDn` | scroll |
| `g` | grid jump: two labelled presses land the cursor anywhere |
| `Esc`, `q` | disarm, keyboard returns to the app |

While the pointer is armed it holds the keyboard, so the app underneath sees no
keys until you disarm it. The bar shows `ptr` while that is the case.

## Fn layer (keyd)

`Fn` + number row = `F1`–`F12`. `Fn`+`hjkl` = arrows, `Fn`+`i`/`m` = page
up/down, `Fn`+`y`/`o` = home/end, `Fn`+arrows = volume and brightness.

**These are placeholders until Phase 0 runs on real hardware.** `scripts/pt35-probe.sh`
plus `sudo evtest` record what each physical key actually emits; only then can
this file be exact. In particular, if the D-pad and gaming buttons enumerate as
a *gamepad* (`ABS_HAT0X`, `BTN_SOUTH`) rather than as keys, neither sway nor
keyd can see them and a small evdev→uinput remapper is needed.
