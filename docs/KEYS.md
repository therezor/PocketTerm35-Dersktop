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
| Start | open or close the menu, from anywhere |
| Select | switch to the next open app |
| Super + Space, Menu key | open the menu |
| Super + b | button mode on or off |
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

There is no tiling. One window owns the screen and the rest wait on their own
workspace: pt35d moves a second window off a workspace that already has one.

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

It is on everywhere except the terminal and the editor, which need the letters.
An app profile sets its own default with `buttons = false`, the bar shows `BTN`
or `TXT`, and Super+b or the Quick menu flips it. The menu drops the grab while
it is open, because it reads the letters itself.

The RP2040 sends the same keycode for the A button and for the `a` key on the
keyboard, from the same USB HID device, so nothing below the compositor can tell
them apart. Button mode is therefore all-or-nothing per app. Separating them for
good needs different keycodes from the keyboard firmware.

## Menu

| control | action |
|---|---|
| D-pad | move |
| A, Enter | open |
| B, Backspace | back |
| X | search, then letters type |
| Y | back to the top menu |
| L / R | page |
| 1-9 | pick that visible row |
| Start, Escape | close |
| Select | close and switch app |
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
