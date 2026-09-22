# Controls

Twelve physical controls. Six of them are letters, so the shell is modal.

| control | key it sends |
|---|---|
| D-pad | arrows |
| A B X Y L R | the literal letters `a b x y l r` |
| Start | `KEY_PAUSE` |
| Select | `KEY_SYSRQ` |

## Global

| control | action |
|---|---|
| Select | open or close the menu, from anywhere |
| Start | toggle button mode |
| Super + Space, Menu key | open the menu |
| Super + Enter | terminal |
| Super + q | close the window |
| Super + w | window switcher |
| Super + 1..9 | workspace |
| Super + Tab | next workspace |
| Super + p | keyboard pointer, `g` for grid jump |
| Super + f | fullscreen toggle |
| Super + r | fit an oversized window to the screen |
| Super + s | screenshot |
| power button | power menu (long press still cuts power) |

There is no tiling. Every window opens fullscreen and owns the screen.

## Button mode

A B X Y L R are the letters `a b x y l r`, so by default they type. Button mode
makes the compositor grab them instead:

| button | action |
|---|---|
| A | Enter |
| B | Escape |
| X | Tab |
| Y | fullscreen toggle |
| L / R | previous / next workspace |

Start toggles it and the bar shows `BTN` or `TXT`. Each app profile sets its own
default in `apps.toml` (`buttons = true`): viewers start in button mode, the
terminal and the editor start in text mode. The menu drops the grab while it is
open, because it reads the letters itself.

## Menu

| control | action |
|---|---|
| D-pad | move |
| A, Start, Enter | open |
| B, Backspace | back |
| X | search, then letters type |
| Y | back to the top menu |
| L / R | page |
| 1-9 | pick that visible row |
| Select, Escape | close |
| touch | tap a tile, a row or a legend pill |

On the window switcher, Y closes the highlighted window. On a quick setting, the
D-pad left and right change the value in place.

## The dock

The top bar is a dock: one slot per open window, the focused one filled. Tap a
slot to switch, the `=` button opens the menu, the `x` button closes the focused
window.

## Keyd

`/etc/keyd/pocketterm35.conf` is installed and the service is left disabled. The
firmware already emits standard keysyms, so there is nothing to remap until a
unit needs it. Capture what a key really sends with `sudo keyd monitor`.
