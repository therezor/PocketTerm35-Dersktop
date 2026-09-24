<div align="center">

# pt35-desktop

**A pocket desktop for the Waveshare PocketTerm35.**

Built for a 640x480 screen, a thumb keyboard and a D-pad.<br>
Real Linux apps, one per screen, no tiny windows.

[![CI](https://github.com/therezor/PocketTerm35-OS/actions/workflows/ci.yml/badge.svg)](https://github.com/therezor/PocketTerm35-OS/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/therezor/PocketTerm35-OS)](https://github.com/therezor/PocketTerm35-OS/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

<img src="docs/screenshots/launcher.png" width="640" alt="The launcher">

</div>

## Why

Raspberry Pi OS Desktop expects a big screen and a mouse. On a 3.5" panel its
windows do not fit and its title bars eat the screen.

pt35-desktop gives every app the whole screen, and puts everything else in one
menu you drive with the D-pad. A mouse works too, built in or plugged in.

<table>
<tr>
<td><img src="docs/screenshots/switcher.png" width="300" alt="Window switcher"></td>
<td><img src="docs/screenshots/app.png" width="300" alt="An app with the taskbar"></td>
</tr>
<tr>
<td align="center">Window switcher, with a picture of each window</td>
<td align="center">One app owns the screen. The taskbar stays on top.</td>
</tr>
<tr>
<td><img src="docs/screenshots/context.png" width="300" alt="Right-click menu"></td>
<td><img src="docs/screenshots/appearance.png" width="300" alt="Appearance settings"></td>
</tr>
<tr>
<td align="center">Right click a row for more</td>
<td align="center">Six colour palettes, applied to GTK and Qt apps too</td>
</tr>
</table>

## Features

- **Launcher.** Every installed app, recents and pins on top. Type to search.*
- **Taskbar.** One icon per open window. Tap one to switch.
- **Window switcher.** Cards with a live picture of each window.
- **Two input modes.** Buttons mode drives menus and apps with the D-pad.
  Mouse mode turns the D-pad into a cursor. R switches.
- **Mouse and touch.** Hover, click, right click, wheel, and breadcrumbs to go back.
- **Quick settings.** Wi-Fi, Bluetooth, volume, brightness*, input mode, touch.
- **Network.** Wi-Fi list and Ethernet.
- **CPU profiles.** Powersave, balanced and performance.
- **Local AI agent.** Optional. darkwire in a terminal, on a model the Pi runs itself.
- **Fast.** Plain Rust, drawn in software. The menu opens in about 75ms on a Pi 5.

\* With the patched keyboard firmware (install step 2).

It is a real graphical desktop. sway draws the screen and XWayland runs X11
apps, so GTK, Qt and X11 programs work as normal.

## Install

You need a PocketTerm35 with a Pi 4B or Pi 5, and **Raspberry Pi OS (Trixie)
64-bit**. Lite is best. Bookworm is not supported.

**1. Install.** On the device:

```sh
curl -fsSL https://raw.githubusercontent.com/therezor/PocketTerm35-OS/main/install.sh | sudo bash
```

This downloads the latest release and installs it with all its dependencies.
It also sets up the screen, touch, login and the udev rules.

**2. Flash the keyboard firmware.** Recommended.

```sh
sudo pt35-kbd flash
```

Stock firmware makes A B X Y L R send the letters `a b x y l r`, so the shell
cannot tell a button from a key. The new keymap gives the buttons keys of their
own, and adds screen brightness control. Undo it with `sudo pt35-kbd restore`.
See [firmware/README.md](firmware/README.md).

**3. Reboot.**

```sh
sudo reboot
```

You log in straight to the desktop.

### Update, remove, build

| to | run |
|---|---|
| update | the install command again |
| remove | `curl -fsSL https://raw.githubusercontent.com/therezor/PocketTerm35-OS/main/install.sh \| sudo bash -s -- --uninstall` |
| build from a checkout | `sudo ./install.sh --from-source` (needs rustup) |
| see what it would do | add `--dry-run` |

Removing puts the old login back and leaves your files in `~/.config` alone.
Reboot after.

The installer also sets up the Files app (yazi) and its icon font. Installing
the `.deb` by hand skips that.

### Local AI agent (optional)

The installer asks whether you want it, and suggests a model for your RAM.
`--ai` says yes without asking, `--ai=MODEL` picks the model, `--no-ai` skips it.
Nothing leaves the device.

| model | download | for | tool calls* | Pi 5 speed** |
|---|---|---|---|---|
| `minicpm5-2b` | 1.5 GB | 4 GB RAM or more | 18/20 | 58 / 8.2 tokens/s |
| `minicpm5-1b` | 0.7 GB | 2 GB RAM | 12/20 | 100 / 20 tokens/s |
| `qwen3-0.6b` | 0.4 GB | 1 GB RAM | 12/20 | 133 / 28 tokens/s |

\* Twenty agent tasks: the right tool with the right arguments.
\*\* `llama-bench`: reading a 512-token prompt / writing the answer. Prompts
read slower as the context fills: 3000 tokens in, the 2B reads 30 tokens/s.
A Pi 4 has no dot-product instructions, so llama.cpp cannot repack the weights
for it and prompts read slower. That is not measured here.

With a 16k context full, the 2B takes 2.1 GB and the 1B (8k) 0.85 GB, which
leaves the desktop room on 4 GB and 2 GB boards. Qwen3 0.6B takes 0.7 GB, more than a
1 GB board has beside the desktop: it runs there on swap, slowly.

darkwire talks to [llama-swap](https://github.com/mostlygeek/llama-swap) on
`127.0.0.1:8080`, which starts llama.cpp with the model on the first request
and frees its RAM after 15 idle minutes. The first answer after that waits for
the model to load. Every agent has thinking off: at these speeds it costs minutes.
`/effort high` in darkwire turns it on for that agent, `/effort off` back.

darkwire is in development, so it is not pinned: setup runs its own installer
for the latest release, and `sudo pt35-ai agent` updates it.

The context is sized to the board: the longest whose cache fits in a fifth of
RAM. The 2B gets 16k tokens on 4 GB, 32k on 8 GB and 64k on 16 GB.

The DarkWire tile opens the Default agent, and `/agent` switches to another.
Each carries only the tools its job needs, one-line tool descriptions and a
short prompt, because the Pi reads about 40 prompt tokens a second:

| agent | role | tools | prompt per step | a test task |
|---|---|---|---|---|
| `default` | universal: files, this Pi, the web | read, ls, find, grep, write, edit, exec, web_search, web_fetch | 2,200 | CPU temperature, 61 s |
| `chat` | minimal: talk, no tools | none | 330 | one answer, 5 s |
| `code` | small exact edits, runs the tests | read, ls, find, grep, edit, write, exec | 1,700 | fix a bug, 52 s |
| `research` | finds out, in the code or on the web, names sources | grep, find, ls, read, web_search, web_fetch, write | 1,900 | where the app's port is set, 75 s |
| `lead` | splits a job and hands it to `code` and `research` | the two helpers only | 800, plus theirs | fix a bug, then a question, 207 s |

Prompt sizes are measured on the 2B, as a turn's total over its steps. `exec`
runs a looking command (`df`, `free`, `journalctl`, `git status` and the like)
at once, asks before anything else, and refuses `sudo`, `git push` and
`pt35ctl power`. darkwire's own 3,350-token prompt takes about 85 seconds to
read cold, which is why none of these use it.

The agents come from `/usr/share/pt35-desktop/ai/darkwire-agents.yaml`. Setup
and every model switch write them into `~/.darkwire/config.yaml`, with the
local model and its context, and a `llamacpp` provider so `/model` lists what
is installed. After that they are yours to edit, there or in darkwire's web
UI. `pt35-ai agents --reset` puts the shipped ones back.

| to | run |
|---|---|
| open it | the DarkWire tile, or `darkwire chat` |
| set it up later | `sudo pt35-ai setup [MODEL]` |
| see the models | `pt35-ai models` |
| switch model | `sudo pt35-ai model MODEL` |
| install or update darkwire | `sudo pt35-ai agent` |
| remove it all | `sudo pt35-ai remove` |

## Controls

| button | Buttons mode | Mouse mode |
|---|---|---|
| D-pad | arrows | move the cursor |
| A | Enter | left click |
| B | Escape | right click |
| X | Tab | scroll up |
| Y | F10 (the app's menu bar) | scroll down |
| L | close the window | close the window |
| R | switch to Mouse mode | switch to Buttons mode |
| Start | menu | menu |
| Select | window switcher | window switcher |

In the menu: A opens, B goes back, X searches, Y pins an app or goes home.
With a mouse: hover selects, click opens, right click shows more, the wheel
scrolls, and the path in the header takes you back.

| keys | what it does |
|---|---|
| `Super`+`Space` | menu |
| `Super`+`Enter` | new terminal |
| `Super`+`1`...`9` | go to that workspace |
| `Super`+`q` | close the window |
| `Super`+`f` | fullscreen (hides the bar) |
| `Super`+`s` | screenshot |
| Fn+Select | screenshot (patched firmware) |

All keys: [docs/KEYS.md](docs/KEYS.md).

## Configure

Defaults live in `/usr/share/pt35-desktop/pt35/`. Put your own copy in
`~/.config/pt35/`: it is merged over the defaults, table by table.

| file | controls |
|---|---|
| `menu.toml` | the menu: pages, apps, commands |
| `apps.toml` | each app: command, workspace, scale, environment |
| `theme.toml` | colours, fonts, sizes, icons |
| `hooks/hook_*` | scripts run at startup, app launch, shutdown |

Settings > Appearance writes `~/.config/pt35/appearance.toml` for you.

An app that is too big for 640x480 gets `scale = 0.75` in `apps.toml`. The app
draws smaller. The desktop does not.

More: [GUI apps](docs/gui-apps.md), [hardware facts](docs/hardware-facts.md).

## If something goes wrong

| problem | try |
|---|---|
| the menu will not open | `Super`+`Space` |
| the buttons do nothing | flash the firmware (install step 2), or touch the screen |
| the whole desktop is stuck | `sudo systemctl restart greetd` over SSH |
| you want the logs | `/run/user/1000/pt35/sway.log` |

The CPU profiles in Settings are tuned for the Pi 5. Performance needs a 27W
supply: the Pi browns out under load on a small one.

## How it works

```
sway (Wayland + XWayland)
 |
 +-- pt35d       session daemon: windows, input modes, hardware, status
 +-- pt35-bar    the 34px taskbar
 +-- pt35-menu   launcher, switcher, settings, power
 +-- pt35ctl     the command every key binding calls
```

No GTK, Qt or GL in the shell itself. It fits a 2 GB Pi.

## Licence

MIT. The device-tree overlays in `boot/` are Waveshare's, unchanged. See
[boot/README.md](boot/README.md).
