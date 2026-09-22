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
| L | context menu | context menu |
| R | window picker | window picker |
| Start | menu | menu |
| Select | switch mode | switch mode |

L sends the keyboard **Menu** key, so the focused app opens its own context
menu. In Buttons mode that is the only right-click there is. sway must not bind
`Menu` itself, and `xkb_options compose:menu` must stay off, or the keysym never
reaches the app.

Nothing closes a window from a shoulder. A button under your index finger is too
easy to catch by accident: closing is `Super`+`q`, the `x` at the right of the
bar, or Y on the window picker.

Holding A in Mouse mode drags: the press and the release are sent separately.
The D-pad repeats faster there (40/s against 8/s) so the cursor crosses the
screen in about a second.

Both modes are sway bindings on the keysyms the patched firmware sends, so
nothing runs in the background and the letters keep typing. The menu takes them
back while it is open, because it reads the same keys itself.

## Keyboard

| key | action |
|---|---|
| Super + Space | open the menu |
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
| D-pad up/down | move |
| D-pad left/right | change a quick setting, or move in a grid |
| A, Enter | open |
| B, Backspace | back |
| X, `/` | search, then letters type |
| Y | back to the top menu |
| L / R | page |
| 1-9 | pick that visible row |
| Start, Escape | close |
| touch | tap a tile, a row, a side button or a legend pill |

Left and Right do **not** go back or open. Back is B. A thumb resting on the
D-pad used to pop a screen or launch the row under the cursor, and nothing on
screen said it would.

On the window switcher, Y closes the highlighted window. Search is left with
Backspace, or with Start, which clears the filter before it closes anything.

## The menu is the desktop

With no window open, the menu comes up on its own and will not close: there is
nothing behind it. Launch something and it goes.

## The taskbar

The top bar is a taskbar: one slot per open window, with its icon and its name,
the focused one filled. Names shrink as windows are added and drop to icons when
there is no room left, but a window never loses its slot. Tap a slot to switch,
the `=` button opens the menu, the `x` button closes the focused window.

The right-hand side is the input mode, the volume and the signal, as icons. A
wired connection draws a full meter rather than an empty Wi-Fi one.

The taskbar is also where a confirmation lands. `pt35ctl` run from a key binding
writes to a stderr nobody reads, so anything worth knowing ("Saved shot.png",
"no audio backend on this system") takes the slot area for a couple of seconds
and then gives it back.

## Icons

Icons come from the installed theme, Papirus by default (`[icons]` in
`theme.toml`). An app names one with `icon` in `apps.toml`, a menu tile with
`icon` in `menu.toml`. Status icons use the theme's symbolic set and are drawn
in the theme colour; app icons are drawn as they were designed. Anything the
theme does not have falls back to the letter in `glyph`.

## What this hardware cannot do

The backlight is a PWM pin on the RP2040 (GP20), not a Linux device: there is no
`/sys/class/backlight` and `pt35ctl brightness` says so. Change it with **Fn and
`-` / `=`** on the keyboard. **Fn**+**C** blanks the screen.

The speaker gain is the same story, on GP18, with Fn and the volume keys. The
volume in the bar and the menu is PipeWire's, which is a different knob.

## Keyd

`/etc/keyd/pocketterm35.conf` is installed and the service is left disabled. The
firmware already emits standard keysyms, so there is nothing to remap until a
unit needs it. Capture what a key really sends with `sudo keyd monitor`.
