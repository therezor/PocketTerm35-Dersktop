# Running GUI apps on a 640×480 screen with no mouse

Filled in as applications are tested on the device. The question for each one is
not "does it start" but "can it be used with a thumb keyboard and a D-pad".

## The app shrinks, not the shell

The panel stays at 640x480, scale 1. Changing the sway output scale would be
global: the bar and the menu would shrink with the app, and every app would
pay for the one that needed it. So the `scale` in a profile is passed to the
app itself, as `GDK_DPI_SCALE` and `QT_SCALE_FACTOR`:

| toolkit | what `scale = 0.75` does |
|---|---|
| Qt 5/6 | shrinks the whole UI, sharp |
| Chromium | use `--force-device-scale-factor=0.75` in `exec` instead |
| GTK3 | shrinks text only, widgets keep their size |
| GTK4 | ignores it; there is no fractional scale below 1 |

Known cost that cannot be avoided: GTK4/libadwaita draws its own headerbar and
the compositor cannot remove it (`gtk-decoration-layout=:` only strips the
buttons; `GTK_CSD=0` is GTK3-only). That is ~46 px of 480 gone in those apps.

`pt35ctl scale 0.75` still changes the output scale by hand, for an app that
fits no other way. Nothing does it automatically.

## Test matrix

| app | toolkit | scale | pointer needed | verdict |
|---|---|---|---|---|
| `foot` | native | 1.0 | no | *(to test)* |
| `imv` | native | 1.0 | no | *(to test)* |
| `mpv` | native | 1.0 | no | *(to test)* |
| `yazi` (in foot) | terminal | 1.0 | no | the Files app |
| `pcmanfm` | GTK3 | 0.75 | yes | usable, text shrinks only |
| `zathura` | GTK3 | 0.75 | no | *(to test)* |
| `chromium` | own | 0.75 | yes | *(to test)* |
| a GTK4 app | GTK4 | 0.75 | yes | *(to test)* |
| a Qt app | Qt6 | 0.75 | yes | *(to test)* |
| an X11-only app | XWayland | 0.75 | yes | *(to test)* |

## When an app does not fit

1. Try `pt35ctl scale 0.75` by hand: the panel gives clients 853x640 logical.
   The shell shrinks with it, so this is a last resort, not a default.
2. `Super`+`r` (`pt35ctl window fit`) drags an oversized window back on screen.
3. Switch to Mouse mode (Select) for controls that only answer a cursor.
4. If it is still unusable, record it here and suggest the terminal alternative.
