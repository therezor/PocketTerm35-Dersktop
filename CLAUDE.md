# pt35-desktop

Desktop environment for the Waveshare PocketTerm35: Pi 4B/5, 640x480 HDMI panel,
67-key thumb keyboard, D-pad, A/B/X/Y, L/R, Start/Select, Goodix GT911 touch.

## Writing style

Applies to chat replies, commit messages, docs and code comments.

- **No em dashes.** Not `—`, not `-` standing in for one. Use a full stop, a comma,
  a colon, or brackets.
- **Say it once.** No restating the request, no summary of what you just wrote.
- **Short words and short sentences.** Split anything past about 25 words.
- **Cut hedging.** Drop "just", "simply", "basically", "essentially", "probably".
- **Comments are minimal.** Say why, not what, and only where the pattern is not
  standard. The code already says what it does.
- Simple English. No yapping.

## Hardware facts

Captured from the board, not guessed. See `docs/hardware-facts.md`.

- Display: HDMI-A-1, native 640x480. No overlay needed for the picture.
- Touch: GT911 on i2c-1 at 0x5d. Use `waveshare-35dpi-5b.dtbo` on both Pi 4B and Pi 5.
- Input: RP2040 at USB `1209:0001`. Plain HID keyboard plus a 5-button HID mouse.
  No gamepad device.
- Buttons: D-pad is arrows. A B X Y L R send F13-F18 with the patched firmware
  in `firmware/`, the letters `a b x y l r` on stock. Select and Start send F21
  and F22 (`XF86TouchpadToggle`, `XF86TouchpadOn`) with the patched firmware,
  so Fn+Select is the real Print Screen (a screenshot) and Fn+Start the real
  Pause, passed to the app. On stock they are `KEY_SYSRQ` and `KEY_PAUSE`, with
  or without Fn. `pt35d` always binds the F21/F22 keys, plus Print and Pause
  until the patched firmware answers on serial. The QWERTY and the buttons are one HID device, so the
  letters cannot be told apart from the keys: that is why the firmware changed.
- No `/sys/class/backlight` and no `/sys/class/power_supply`. Brightness is a PWM
  pin on the RP2040 (GP20). The pt35 firmware takes `B<0-100>` / `B?` on its
  USB console (`/dev/ttyACM0`) and answers `PT35 B=<n>`, with a 5% floor;
  `pt35d`'s `backlight.rs` drives it. Stock firmware never answers, and then
  brightness is Fn `-` / `=` only. That answer is also how the shell knows the
  pt35 firmware is on (`Status::pt35_firmware`): letters stop being buttons and
  the launcher searches as you type. Battery has no such path. Never show a
  gauge or a row that can only read `--`.
- Network: `eth0` (the Pi's RJ45) and `wlan0`, both NetworkManager. A cable
  with link wins: `Status::ethernet` is set, the bar shows the wired icon, and
  the quick panel and Network screen gain an Ethernet row. No cable, no row.

## Input model

Two modes, nothing else: Buttons (default) and Mouse.

Buttons mode is the D-pad as arrows, A Enter, B Escape, X Tab, Y F10 (the
app's menu bar: File, Edit, Help in GTK, Firefox, LibreOffice; quit in htop
and mc). Fullscreen is `$mod+f` only.
Mouse mode is the D-pad moving the cursor, A and B the clicks, X and Y the
wheel. In both: L closes the focused window, R switches mode, Start is the
menu and Select is the window switcher. Each shoulder means one thing
everywhere: no list pages with L or R.

In Mouse mode the menu keeps the cursor: `pt35d` leaves the D-pad and the
clicks bound (`modes::menu_binds`). The menu is a normal mouse UI in either
mode, so an external mouse works in Buttons mode too. Pointer motion selects
the row under it (an enter does not, so a menu opening under a resting cursor
keeps its selection). A click opens, the wheel scrolls, and a right click opens
the row's menu: Open and Pin on a launcher app, Switch to, Close and Close all
on a switcher card. Back with a mouse is the header's breadcrumbs, every name
before the last a link. The scrollbar is a target too: a click on the track
jumps there, a drag moves the list.

X and Y turn a wheel on `pt35d`'s uinput mouse (`vkbd.rs`, `pt35ctl wheel`):
sway 1.10's `seat cursor press button4` sends a pointer frame with no scroll
in it. sway also holds back the motion of `seat cursor move` while a button is
down, so A plus the D-pad cannot drag; a real mouse or a finger can.

Enter, Escape, Tab and F10 from the face buttons go through `pt35d`'s uinput
keyboard (`vkbd.rs`), not `wtype`: XWayland misreads wtype's keymap and every
key arrives as Escape.

L and R are sway bindings on the firmware's keysyms (`pt35d`'s `modes.rs`),
dropped while the menu is open because a sway binding beats any surface and the
menu reads the same keys. Never take a binding off a key that may be held: sway 1.10
segfaults when it runs the freed binding on release or on key repeat. So
`bind_mode` changes only the bindings that differ, and Start and Select are
`--no-repeat`. The menu then does the same job itself: R switches
mode, L closes the card under the cursor on the switcher and nothing elsewhere,
because the window behind the menu is out of sight. Start and Select are in the
sway config, so no mode can lose them.

The menu is modal, for a stock unit where the letters are the buttons: nav mode
(A opens, B back, X search, Y home) and filter mode. The D-pad sideways changes
a quick setting and nothing else. Back is B, and from the top screen back means
out, so no legend names Start: it is the same exit in one press.

Start is bound by `pt35d`, not the sway config. A sway binding beats any
surface, and the menu has to see the key to know what closing means on the
screen you are looking at: on an empty desktop it goes to the top screen rather
than leaving you with nothing. `$mod+space` stays bound in the config as the
way back in if that handover ever fails.

The window switcher is a horizontal strip of cards in taskbar order, Launcher
first. Left and right move. It opens on the window you are in, centred and
highlighted. B on it returns to the app. X closes every
window, behind a confirmation.

On the launcher, typing starts a search (`type_to_search`, off for stock
firmware) and X does too. Matches rank by name prefix, word start, anywhere,
then letters in order, over the label and the app's id or command. Y pins the
row: pins sit above the recents, in `~/.local/state/pt35/pinned`.

The launcher cannot be closed from inside it. The picker's first tile is the
Launcher: A opens it, Y and X leave it alone. At the launcher's top screen B does
nothing, and the bar hides its close button while the menu is up. You leave by
picking an app or a window, or with Start.

## Staying in step with sway

`pt35d` subscribes to sway's `window` and `workspace` events (`events.rs`), on a
connection of its own, and rebuilds the window list on each one. Events are
coalesced at 50ms: `window::title` fires on every prompt redraw. The 2 second
poll is for hardware only.

`kill` is a request, not a deletion: sway answers it before the client has gone.
So closing drops the window from the list locally and lets the `window::close`
event confirm it. An app that refuses to close reappears, which is correct.

With nothing open on the workspace in front, the menu is the desktop, like a
phone's home screen (`Status::workspace_empty`): it opens on its own and refuses
to close. Windows on other workspaces do not count, so an app that exits leaves
you on the launcher, not on an empty workspace. A launch suppresses that for
five seconds, until the window maps, and the menu steps aside the moment a
window appears in front.

## Layout

640x480 leaves no room for chrome that is not earning its place.

- No tiling. One window owns the screen; pt35d moves a second window on a
  workspace to a free one. Not sway `fullscreen`, which would hide the bar.
  Dialogs float over their app and are never moved. A portal's file chooser
  has no parent sway can see, so it is floated by name in the sway config.
- The bar rises to the overlay layer while the menu is up
  (`App::raised`): sway hides the top layer under a fullscreen window. It
  moves only with no button or finger down: sway 1.10 segfaults when a surface
  changes layer under its own pointer grab.
- The top bar is a taskbar: one icon per open window, tap to focus, the skull
  menu button left, close button right. The launcher is a window that is
  always there: the skull brings it to the front and never closes it, and a
  dock slot tapped over the menu takes the menu down. The mode icon toggles
  the mode; volume, network and the clock open Audio, Network and Quick.
  Bar and menu act on button release, not press: in Mouse mode a click is a
  held A, and a mode switch rebinds A. No names by default (`dock_labels`):
  they crowd out by the third window. A window never loses its slot, and one
  with no icon of its own gets the generic app icon.
  No "nothing open" text: with no windows the menu covers the bar.
- A toast takes the slot area for a couple of seconds. It is the only feedback a
  key binding gets.
- Bar 34px, menu header 46px, hint bar 46px, list rows 50px, tiles 86px.
- Corners are 2px. Squared, not rounded.
- Motion is two things, each about 0.1s: the selection glides, a new screen
  settles. The draw loop runs at 60fps only while one is moving and sleeps
  otherwise (`App::animating`). `[menu] animations = false` turns both off.
- Performance comes first. The menu is a new process per open, so nothing slow
  may run before its first frame: measure it (about 75ms on a Pi 5). An icon
  miss once walked every theme folder and cost 3.5s.
- Mint on charcoal, mono for readouts and sans for labels. Geometry and colour
  live in `config/pt35/theme.toml`. Do not hardcode either in a widget.
  Settings > Appearance writes `~/.config/pt35/appearance.toml`, layered between
  the shipped theme and the user's `theme.toml`. A palette sets every colour and
  the wallpaper. GTK and Qt apps follow `[apps] color_scheme` through gsettings.
- Icons come from the installed Papirus theme (`pt35-ui`'s `icon.rs`), tinted
  from the symbolic set for status and full colour for apps. Nothing depends on
  it: every icon falls back to the `glyph` letter or a drawn meter.
- An app profile's workspace is applied with a sway `assign` rule at startup, not
  by switching workspace before spawning: that is a race a slow app loses.
- The output stays at scale 1. A profile's `scale` goes to the app as
  `GDK_DPI_SCALE` / `QT_SCALE_FACTOR`, so the shell never shrinks with it.

## Local AI agent

Optional, and all in `scripts/pt35-ai`. darkwire (`darkwire chat -p llamacpp`)
talks to llama-swap on `127.0.0.1:8080` (`pt35-ai.service`), which runs the
prebuilt llama.cpp in `/opt/pt35-ai`. Models are in `/var/lib/pt35-ai/models`.

- llama.cpp, llama-swap and each model file are pinned by version and sha256.
  darkwire is in development and is not pinned: its own installer decides.
- The quants were picked by measurement on the Pi 5, not by reputation: tool
  calls, perplexity, prompt and generation speed. Measure again before
  changing one. Q4_0 repacks for the A76 and reads prompts 65% faster than
  Q4_K_M on the 2B, but at Q4_0 Qwen3 0.6B stops calling tools.
- The KV cache is f16. q8_0 halves it, but 3000 tokens deep it reads prompts
  2-5x slower on the Pi 5, so only a 1 GB board gets it.
- No speculative draft model. MiniCPM5's DSpark draft ships as BF16, which the
  A76 cannot run fast, and even quantized it slowed prose by a quarter. The
  n-gram draft costs nothing when it misses.
- The installer asks on `/dev/tty` before the build, and does the download
  last. Its RAM limits match `recommend` in `pt35-ai`.
- The DarkWire tile runs `darkwire chat` itself, on the Default agent. The
  five agents in `config/ai/darkwire-agents.yaml` go into
  `~/.darkwire/config.yaml` at setup and at every model switch (`pt35-ai
  agents`, run as the desktop user), with a `llamacpp` provider: darkwire
  lists models only from the providers its config names.
- The context is the longest whose KV cache fits in a fifth of RAM
  (`model_ctx`), and darkwire's `contextWindowTokens` is set to match.
- An agent's prompt is its speed. darkwire's default agent sends 3,350 tokens,
  about 85 s to read cold on the 2B. Give an agent only the tools its job
  needs, shorten `toolPrompts`, and keep `{{tag}}` and `{{time}}` out of the
  cached half of the prompt.
- The lead has no tools of its own: given file tools, the 2B used them rather
  than delegate. It starts each hand-off with the user's own words.
- Small models copy what the prompt shows: write commands as argv arrays,
  `["df","-h"]`. Written as `df -h`, the model sends it as one string.

## Build and test

```sh
cargo test                                                  # host
PKG_CONFIG_ALLOW_CROSS=1 cargo clippy --target aarch64-unknown-linux-gnu \
  --workspace --all-targets -- -D warnings                  # what CI runs
```

CI runs on arm64: fmt, clippy with `-D warnings`, tests, shellcheck, overlay checksums.

## Device loop

The board is a Pi 5 at `rezor@192.168.1.10` with passwordless sudo and the repo in
`~/pt35-desktop`.

```sh
ssh rezor@192.168.1.10 'cd ~/pt35-desktop && git pull -q && \
  su -l rezor -c "cd ~/pt35-desktop && cargo build --release --workspace" && \
  sudo install -m755 target/release/pt35-menu /usr/bin/'
ssh rezor@192.168.1.10 'export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1; \
  pt35ctl menu toggle; grim /tmp/shot.png'
```

Config files go to `/usr/share/pt35-desktop/`. Binaries go to `/usr/bin/`.
`sudo systemctl restart greetd` restarts the whole session.

## Reference

`github.com/trailcurrentoss/TrailCurrentTracer` runs on the same hardware. Its
`docs/controls.md` and `tracerd/keymap.default.json` are the source of the button
map. Its design is the visual target: tile grid launcher, coloured button pills
in the hint bar, no battery gauge.
