# Controls

Twelve physical controls. Six of them are letters, so the shell is modal.

| control | key it sends |
|---|---|
| D-pad | arrows |
| A B X Y L R | `F18 F17 F15 F16 F13 F14` (`XF86Launch9`, `XF86Launch8`, `XF86Launch6`, `XF86Launch7`, `XF86Tools`, `XF86Launch5`) |
| Start | `KEY_PAUSE` |
| Select | `KEY_SYSRQ` |

That is with the patched keyboard firmware in [`firmware/`](../firmware/).
Stock, the six face buttons send the letters `l r x y b a` and the shell has to
grab them, which is what "button mode" below is for.

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

## The face buttons inside an app

| button | action |
|---|---|
| A | Enter |
| B | Escape |
| X | Tab |
| Y | fullscreen toggle |
| L / R | previous / next app |

The compositor holds these all the time, including in the terminal: with the
patched firmware they are not letters, so nothing is lost. The menu drops them
while it is open, because it reads the same keys itself.

`Super`+`b` or Quick > Buttons turns the grab off, and the bar shows `BTN` or
`TXT`. On stock firmware that toggle is the only way to use the buttons as
buttons, and it costs you the letters `a b x y l r` while it is on.

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
