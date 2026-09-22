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
    pt35d ───────┼─ session daemon: sway IPC, battery, backlight, volume,
                 │  CPU profile, idle policy, hooks, status feed
    pt35-bar ────┤  18px status strip (layer-shell, software-rendered)
    pt35-menu ───┤  fullscreen launcher / window switcher / settings / power
    pt35-pointer ┤  D-pad cursor + two-press grid jump (wlr-virtual-pointer)
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

`Super` is whatever key keyd maps to it (see `/etc/keyd/pocketterm35.conf`).

| key | what it does |
|---|---|
| `Super`+`Space` | open the menu — the hub for everything |
| `Super`+`Enter` | new terminal |
| `Super`+`1`…`9` | go to that workspace (one app each) |
| `Super`+`Tab` | next workspace |
| `Super`+`q` | close the window |
| `Super`+`p` | arm the keyboard pointer (`g` inside it = grid jump) |
| `Super`+`f` | true fullscreen (hides the bar) |
| `Super`+`r` | drag an oversized window back on screen |
| `Super`+`s` | screenshot |

In the menu: D-pad or arrows move, `Enter` or `Right` activates, `1`–`9` jump
straight to a row, typing filters, `Backspace`/`Left` goes up a level, `Esc`
closes. See [docs/KEYS.md](docs/KEYS.md) for the full map.

## Configuring it

System defaults live in `/usr/share/pt35-desktop/pt35/`; anything you drop in
`~/.config/pt35/` is merged over them, table by table.

| file | what it controls |
|---|---|
| `menu.toml` | the whole menu tree: submenus, apps, commands, `pt35ctl` actions |
| `apps.toml` | per-app profile: command, workspace, output scale, pointer policy, env |
| `theme.toml` | colours, font sizes, bar height, menu row height, pointer speed |
| `~/.config/pt35/hooks/hook_*` | run on startup, low battery, app launch, shutdown |

The one knob that matters most is `scale` in `apps.toml`. At `1.0` the panel is
640×480; at `0.75` clients see an ~853×640 logical surface, which is what makes
GTK4 and Qt dialogs fit. `pt35d` switches it as you move between workspaces.

## Status

Written against the hardware facts in [docs/hardware-facts.md](docs/hardware-facts.md).
Phases 0 and 1 of the plan (probe, boot config, session, keymap) and the Rust
shell (daemon, bar, menu, pointer) are implemented; the on-device verification
pass is what turns the placeholders in `keyd/pocketterm35.conf` into real key
codes. Run `sudo /usr/share/pt35-desktop/pt35-probe.sh > hardware-facts.md` on
the unit and the rest follows from there.

## Licence

MIT. The device-tree overlays under `boot/` are Waveshare's, redistributed
unmodified; see [boot/README.md](boot/README.md).
