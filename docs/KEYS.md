# Controls

Twelve physical controls. Six of them are letters, so the shell is modal.

| control | key it sends |
|---|---|
| D-pad | arrows |
| A B X Y L R | `F18 F17 F15 F16 F13 F14` (`XF86Launch9`, `XF86Launch8`, `XF86Launch6`, `XF86Launch7`, `XF86Tools`, `XF86Launch5`) |
| Start | `KEY_F22` (`XF86TouchpadOn`); Fn+Start is `KEY_PAUSE` |
| Select | `KEY_F21` (`XF86TouchpadToggle`); Fn+Select is `KEY_SYSRQ`, a screenshot |

That is with the patched keyboard firmware in [`firmware/`](../firmware/).
Stock, the six face buttons send the letters `l r x y b a` and the shell has to
grab them, which is what "button mode" below is for.

## Two modes

Buttons mode is the default. Mouse mode exists because some GUI controls only
answer a cursor. R switches between them, a toast says which, and so does the
bar.

The menu works like any desktop with a mouse, in either mode and with an
external mouse too: pointing at a row selects it, a click opens it, a right
click opens its menu (Open and Pin on a launcher app, Switch to, Close and
Close all on a switcher card) and the wheel scrolls. The header is the way
back: `PT35 < SETTINGS < SYSTEM`, and every name before the last is a link.

| control | Buttons | Mouse |
|---|---|---|
| D-pad | arrows | move the cursor |
| A | Enter | left click |
| B | Escape | right click |
| X | Tab | scroll up |
| Y | F10: the app's menu bar | scroll down |
| L | close window | close window |
| R | switch mode | switch mode |
| Start | menu | menu |
| Select | window switcher | window switcher |

L closes the focused window and R switches mode, in both modes and in the
menu. On the window switcher L closes the card under the cursor; on other menu
screens it does nothing, because the window is hidden behind the menu.

Closing is L, `Super`+`q`, the `x` at the right of the bar, or Y on the window
switcher. L does not repeat, so holding it closes one window, not a row.

Holding A and pressing the D-pad does not drag: sway holds back the cursor's
motion while a button is down. A real mouse or a finger drags. The D-pad
repeats faster in Mouse mode (40/s against 8/s) so the cursor crosses the
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
| B, Backspace | back; on the screen the menu opened on, back to your app |
| X, `/`, or just type (launcher) | search, then letters type |
| Y | back to the top menu; on a launcher row, pin or unpin |
| Select | window switcher; on the switcher, close it |
| R | switch Buttons / Mouse mode |
| 1-9 | pick that visible row |
| Start, Escape | close |
| touch | tap a tile, a row, a side button or a legend pill |

B is the way out: back one screen. The launcher itself is never closed from
inside: you leave it by picking something, or with Start. On the window
switcher, X closes every window and asks first, and L or Y closes one.

Left and Right never navigate. Back is B.

On the window switcher, Y closes the highlighted window. Search is left with
Backspace, or with Start, which clears the filter before it closes anything.

## The menu is the desktop

With no window open, the menu comes up on its own and will not close: there is
nothing behind it. Start and B take you back to the top screen instead. Launch
something and it goes.

## The taskbar

The top bar is a taskbar: one slot per open window, with its icon and its name,
the focused one filled. Names shrink as windows are added and drop to icons when
there is no room left, but a window never loses its slot. Tap a slot to switch, even with
the menu up. The skull brings the launcher to the front and never closes it,
the `x` button closes the focused window.

The right-hand side is the input mode, the volume and the signal, as icons. A
cable in wins over Wi-Fi and draws the wired icon rather than an empty Wi-Fi
meter. Each one is a button: the mode icon switches mode, volume opens Audio,
the signal opens Network, and the clock opens Quick settings.

The taskbar is also where a confirmation lands. A key binding has no other way
to answer you, so anything worth knowing ("Saved shot.png", "no audio backend on
this system") takes the slot area for a couple of seconds and then gives it
back.

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
