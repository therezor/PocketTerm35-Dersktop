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

## Two modes

Buttons mode is the default. Mouse mode exists because some GUI controls only
answer a cursor. Select switches between them and the bar says which you are in.

| control | Buttons | Mouse |
|---|---|---|
| D-pad | arrows | move the cursor |
| A | Enter | left click |
| B | Escape | right click |
| X | Tab | scroll up |
| Y | fullscreen toggle | scroll down |
| L / R | previous / next app | previous / next app |
| Start | menu | menu |
| Select | switch mode | switch mode |

Holding A in Mouse mode drags: the press and the release are sent separately.
The D-pad repeats faster there (40/s against 8/s) so the cursor crosses the
screen in about a second.

Both modes are sway bindings on the keysyms the patched firmware sends, so
nothing runs in the background and the letters keep typing. The menu takes them
back while it is open, because it reads the same keys itself.

## Keyboard

| key | action |
|---|---|
| Super + Space, Menu key | open the menu |
| Super + m | switch mode |
| Super + Enter | terminal |
| Super + q | close the window |
| Super + w | window switcher |
| Super + 1..9 | workspace |
| Super + Tab | next workspace |
| Super + f | fullscreen toggle |
| Super + r | fit an oversized window to the screen |
| Super + s | screenshot |
| power button | power menu (long press still cuts power) |

There is no tiling. One window owns the screen and the rest wait on their own
workspace: pt35d moves a second window off a workspace that already has one.

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
