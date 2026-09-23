# Controls

## Buttons

| button | Buttons mode | Mouse mode |
|---|---|---|
| D-pad | arrows | move the cursor |
| A | Enter | left click |
| B | Escape | right click |
| X | Tab | scroll up |
| Y | F10: the app's menu bar | scroll down |
| L | close the window | close the window |
| R | switch to Mouse mode | switch to Buttons mode |
| Start | menu | menu |
| Select | window switcher | window switcher |

Buttons mode is the default. Mouse mode is for apps that only answer a cursor.
R switches, and the bar shows which mode you are in.

L does not repeat, so holding it closes one window, not a row. The D-pad
repeats faster in Mouse mode (40 a second against 8), so the cursor crosses the
screen in about a second.

Holding A and pressing the D-pad does not drag: sway holds back the cursor's
motion while a button is down. A real mouse or a finger can drag.

## The menu

| control | what it does |
|---|---|
| D-pad up/down | move |
| D-pad left/right | change a quick setting, or move in a grid |
| A, Enter | open |
| B, Backspace | back one screen |
| X, `/`, or type on the launcher | search |
| Y | pin or unpin a launcher app; on other screens, back to the top |
| 1-9 | pick that row |
| Select | window switcher; on the switcher, close it |
| R | switch mode |
| Start, Escape | close the menu |

On the window switcher: left and right move, A switches, B goes back to your
app, Y and L close one window, X closes all of them (it asks first).

The launcher never closes from inside. You leave it by picking an app or a
window, or with Start. With no window open, the menu is the desktop and stays.

### With a mouse

The menu works like any desktop, in either mode, with a plugged-in mouse too.

| mouse | what it does |
|---|---|
| move | selects the row under the pointer |
| click | opens it |
| right click | the row's menu: Open and Pin on an app, Switch to, Close and Close all on a window |
| wheel | scrolls |
| scrollbar | click to jump, drag to move |
| header | `PT35 < SETTINGS < SYSTEM`: click a name to go back to it |

Touch works the same way: tap a row, a tile or a legend pill.

## The taskbar

One icon per open window, the focused one filled. Tap one to switch, even with
the menu up. The skull brings the launcher to the front. The red `x` closes the
focused window.

On the right: input mode, volume, network and the clock. Each is a button. The
mode icon switches mode, volume opens Audio, network opens Network, the clock
opens Quick settings. A plugged-in cable shows the wired icon.

A key binding has no other way to answer you, so its message shows in the
taskbar for a couple of seconds: "Saved shot.png", "no audio".

## Keyboard shortcuts

| keys | what it does |
|---|---|
| Super + Space | menu |
| Super + w | window switcher |
| Super + m | switch mode |
| Super + Enter | terminal |
| Super + q | close the window |
| Super + 1..9 | go to that workspace |
| Super + Tab | next workspace |
| Super + f | fullscreen (hides the bar) |
| Super + r | pull an oversized window back on screen |
| Super + s | screenshot |
| Fn + Select | screenshot |
| power button | power menu; press again to close it |

There is no tiling. One window owns the screen, and each app gets a workspace
of its own.

## Keyboard firmware

The patched firmware in [`firmware/`](../firmware/) gives the buttons keys of
their own. Flash it with `sudo pt35-kbd flash`.

| button | patched firmware sends | stock sends |
|---|---|---|
| A B X Y | F18 F17 F15 F16 | `a b x y` |
| L R | F13 F14 | `l r` |
| Start | F22 (Fn+Start: Pause) | Pause |
| Select | F21 (Fn+Select: Print) | SysRq |

On stock firmware a button and its letter are the same key, so:

- Start and Select work.
- In the menu the letters work as the buttons, and typing does not search.
- Outside the menu, A B X Y L R type their letters.
- Mouse mode moves the cursor but cannot click. Switch mode with Super + m or
  the mode icon.
- Brightness is Fn with `-` and `=` only.

## Brightness and volume

With the patched firmware, brightness is a slider in Quick settings and
`pt35ctl brightness`. On stock it is Fn with `-` and `=`. Fn + C blanks the
screen.

The speaker gain is on the keyboard too: Fn and the volume keys. The volume in
the bar and the menu is PipeWire's, a separate knob.

## keyd

`/etc/keyd/pocketterm35.conf` is installed and the service is left off. The
firmware already sends standard keys. See what a key really sends with
`sudo keyd monitor`.
