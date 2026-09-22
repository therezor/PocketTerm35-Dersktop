# pt35-desktop

A desktop environment for the [Waveshare PocketTerm35](https://www.waveshare.com/pocketterm35.htm):
a Raspberry Pi 4B/5 handheld with a 3.5" **640×480** panel, a 67-key thumb
keyboard and a D-pad, and no mouse.

Stock Raspberry Pi OS Desktop is unusable at that size — it assumes a pointer,
1000-pixel-wide windows and title bars that eat a tenth of the screen.
pt35-desktop replaces it with one fullscreen app per workspace, a hierarchical
menu you drive with the D-pad, and a keyboard-driven cursor for the GUI
controls that still insist on a mouse.

It is a **real graphical shell**: sway does the compositing, with XWayland, so
GTK, Qt and X11 applications run as normal windows. Terminal apps are the
default where a good one exists, because they are lighter and read better on a
3.5" screen — not because that is all this can run.

```
                sway (Wayland + XWayland, Raspberry Pi wlroots)
                 │
    pt35d ───────┼─ session daemon: sway IPC and events, battery, backlight,
                 │  volume, CPU profile, hooks, status feed
    pt35-bar ────┤  34px taskbar and status strip (layer-shell, software-rendered)
    pt35-menu ───┤  fullscreen launcher / window switcher / settings / power
    pt35ctl ─────┘  the CLI every key binding and hook calls
```

Everything pt35-desktop adds is Rust, software-rendered, and sized for a 2 GB
Pi: no GTK, no Qt, no GL, no async runtime.

## Install

Raspberry Pi OS **(Trixie) 64-bit**, Lite recommended:

```sh
curl -fsSL https://raw.githubusercontent.com/therezor/PocketTerm35-Dersktop/main/install.sh | sudo bash
sudo reboot
```

Until the first release is tagged there is nothing to download, so the script
clones the repo and builds it on the device instead (10–40 minutes on a Pi 4);
once a `v*` tag exists, CI publishes `pt35-desktop_arm64.deb` and the same
command installs it in seconds.

The installer refuses Bookworm (its `sway` is not rebuilt against the Raspberry
Pi `libwlroots`), warns if the `sway` candidate is not an `+rpt` build, and
keeps a backup of `config.txt`. To undo everything:

```sh
sudo ./install.sh --uninstall
```

From a checkout, `sudo ./install.sh` builds and installs from source instead of
downloading a release.

## Using it

The keyboard has its own `Super`, next to the right Alt. `keyd` is installed but
its service is left disabled: the firmware already sends standard keysyms.

| key | what it does |
|---|---|
| `Start` | open or close the menu, the hub for everything |
| `Select` | switch between Buttons mode and Mouse mode |
| `L` | the keyboard Menu key: the focused app's own context menu |
| `R` | open the window picker |
| `Super`+`Space` | open the menu |
| `Super`+`m` | switch between Buttons mode and Mouse mode |
| `Super`+`Enter` | new terminal |
| `Super`+`1`…`9` | go to that workspace (one app each) |
| `Super`+`Tab` | next workspace |
| `Super`+`q` | close the window |
| `Super`+`f` | true fullscreen (hides the bar) |
| `Super`+`r` | drag an oversized window back on screen |
| `Super`+`s` | screenshot |

Nothing closes a window from a shoulder button: that is `Super`+`q`, the `x` at
the right of the bar, or `Y` on the window picker.

Close the last window and the menu takes the screen. It is the desktop, so it
stays until something is open behind it.

In the menu: D-pad or arrows move, `A` or `Enter` activates, `1`–`9` jump
straight to a row, `X` or typing filters, `B`/`Backspace` goes up a level, `Esc`
closes. Left and Right change a quick setting in place and never navigate. See
[docs/KEYS.md](docs/KEYS.md) for the full map.

## Configuring it

System defaults live in `/usr/share/pt35-desktop/pt35/`; anything you drop in
`~/.config/pt35/` is merged over them, table by table.

| file | what it controls |
|---|---|
| `menu.toml` | the whole menu tree: submenus, apps, commands, `pt35ctl` actions |
| `apps.toml` | per-app profile: command, workspace, toolkit scale, env |
| `theme.toml` | colours, font sizes, bar height, menu row height, icon theme |
| `~/.config/pt35/hooks/hook_*` | run on startup, low battery, app launch, shutdown |

The one knob that matters most is `scale` in `apps.toml`. At `1.0` the panel is
640×480; at `0.75` clients see an ~853×640 logical surface, which is what makes
GTK4 and Qt dialogs fit. It is handed to the app as `GDK_DPI_SCALE` and
`QT_SCALE_FACTOR`, so the shell does not shrink with it. The output itself stays
at scale 1; `pt35ctl scale` changes that, and the shell shrinks with it.

## Status

Written against the hardware facts in [docs/hardware-facts.md](docs/hardware-facts.md).
Phases 0 and 1 of the plan (probe, boot config, session, keymap) and the Rust
shell (daemon, bar, menu) are implemented; the on-device verification
pass is what turns the placeholders in `keyd/pocketterm35.conf` into real key
codes. Run `sudo /usr/share/pt35-desktop/pt35-probe.sh > hardware-facts.md` on
the unit and the rest follows from there.

## Licence

MIT. The device-tree overlays under `boot/` are Waveshare's, redistributed
unmodified; see [boot/README.md](boot/README.md).
