# GUI apps on a 640x480 screen

Any Wayland or X11 app runs. The question is whether it fits the screen and
works with a D-pad and a thumb keyboard.

## The app shrinks, not the shell

The panel stays at 640x480, scale 1. An app that is too big gets a `scale` in
`apps.toml`. It goes to the app as `GDK_DPI_SCALE` and `QT_SCALE_FACTOR`, so
the bar and the menu keep their size.

| toolkit | what `scale = 0.75` does |
|---|---|
| Qt 5 and 6 | shrinks the whole app, sharp |
| Chromium | ignores it: put `--force-device-scale-factor=0.75` in `exec` |
| GTK3 | shrinks the text only |
| GTK4 | ignores it |

GTK4 and libadwaita apps draw their own title bar, and nothing can remove it.
That costs about 46px of the 480.

## The shipped apps

The launcher lists these from `apps.toml`. Anything not installed says so when
you pick it. Install the lot with:

```sh
sudo apt install mousepad vlc imv mpv chromium galculator
```

| app | runs | scale | notes |
|---|---|---|---|
| Terminal | `foot` | 1.0 | opens with a system summary |
| Files | `yazi` in foot | 1.0 | installed by `install.sh` |
| Editor | `mousepad` | 0.75 | GTK3 |
| Monitor | `htop` in foot | 1.0 | |
| Disk usage | `ncdu` in foot | 1.0 | |
| Music | `vlc` | 0.75 | Qt |
| Images | `imv` | 1.0 | opens `~/Pictures` |
| Video | `mpv` | 1.0 | fullscreen |
| Browser | `chromium` | 0.75 | set in its own flags |
| Calculator | `galculator` | 0.75 | GTK3 |

Every other app with a `.desktop` file shows in the launcher too
(`show_desktop_entries` in `theme.toml`).

## When an app does not fit

1. Give it `scale = 0.75` in `~/.config/pt35/apps.toml`.
2. `Super`+`r` pulls an oversized window back on screen.
3. Press R for Mouse mode, for controls that only answer a cursor. Y in
   Buttons mode opens the app's menu bar (F10) in GTK apps, Firefox and
   LibreOffice.
4. `pt35ctl scale 0.75` shrinks the whole screen, shell included, as a last
   resort. `pt35ctl scale 1.0` puts it back.
