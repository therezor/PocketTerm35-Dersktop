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
- Buttons: D-pad is arrows. Start is `KEY_PAUSE`, Select is `KEY_SYSRQ`. A B X Y
  L R send F13-F18 with the patched firmware in `firmware/`, the letters
  `a b x y l r` on stock. The QWERTY and the buttons are one HID device, so the
  letters cannot be told apart from the keys: that is why the firmware changed.
- No `/sys/class/backlight` and no `/sys/class/power_supply`. Brightness is a PWM
  pin on the RP2040 (GP20), reachable only with Fn and `-` / `=` on the keyboard.
  Battery is the same story. Never show a gauge or a row that can only read `--`.

## Input model

Two modes, nothing else: Buttons (default) and Mouse.

Buttons mode is the D-pad as arrows, A Enter, B Escape, X Tab, Y fullscreen.
Mouse mode is the D-pad moving the cursor, A and B the clicks, X and Y the
wheel. In both: L sends the keyboard Menu key so the app opens its own context
menu, R is the window picker, Start is the menu and Select switches mode.

Nothing closes a window from a shoulder. Closing is `$mod+q`, the bar's `x`, or
Y on the window picker. A button under the index finger is too easy to catch by
accident to throw away what is on screen.

L only works because sway does not bind `Menu` and `xkb_options compose:menu` is
off. Either one would eat the keysym before the app saw it.

L and R are sway bindings on the firmware's keysyms (`pt35d`'s `modes.rs`),
dropped while the menu is open because a sway binding beats any surface and the
menu reads the same keys. Start and Select are in the sway config instead, so no
mode can lose them.

The menu itself is still modal, for a stock unit where the letters are the
buttons: nav mode (A opens, B back, X search, Y home) and filter mode. The D-pad
sideways changes a quick setting and does nothing else. It is not Back: B is.

## Staying in step with sway

`pt35d` subscribes to sway's `window` and `workspace` events (`events.rs`) and
rebuilds the window list on each one. The 2 second poll is for hardware only.
Polling for windows is what left a closed window sitting in the dock.

`kill` is a request, not a deletion: sway answers it before the client has gone.
So closing drops the window locally and lets the event put the truth back. An app
that refuses to close reappears, which is what you want it to do.

With nothing open the menu is the desktop: it opens on its own and refuses to
close. A launch holds that off for five seconds, or the menu lands on top of
every app you start.

## Layout

640x480 leaves no room for chrome that is not earning its place.

- No tiling. One window owns the screen; pt35d moves a second window on a
  workspace to a free one. Not sway `fullscreen`, which would hide the bar.
- The top bar is a taskbar: one slot per open window carrying its icon and its
  name, tap to focus, menu button left, close button right. Names shrink and
  then drop to icons as windows are added, but a window never loses its slot.
  There is no "nothing open" text: with no windows the menu covers the bar.
- A toast takes the slot area for a couple of seconds. It is the only feedback a
  key binding gets, since `pt35ctl` from a binding writes to a stderr nobody
  reads.
- Bar 34px, menu header 46px, hint bar 46px, list rows 50px, tiles 86px.
- Corners are 2px. Squared, not rounded.
- Mint on charcoal, mono for readouts and sans for labels. Geometry and colour
  live in `config/pt35/theme.toml`. Do not hardcode either in a widget.
- Icons come from the installed Papirus theme (`pt35-ui`'s `icon.rs`), tinted
  from the symbolic set for status and full colour for apps. Nothing depends on
  it: every icon falls back to the `glyph` letter or a drawn meter.
- An app profile's workspace is applied with a sway `assign` rule at startup, not
  by switching workspace before spawning: that is a race a slow app loses.
- The output stays at scale 1. A profile's `scale` goes to the app as
  `GDK_DPI_SCALE` / `QT_SCALE_FACTOR`, so the shell never shrinks with it.

## Build and test

```sh
cargo test                                                  # host
PKG_CONFIG_ALLOW_CROSS=1 cargo clippy --target aarch64-unknown-linux-gnu \
  --workspace --all-targets -- -D warnings                  # what CI runs
```

CI runs on arm64: fmt, clippy with `-D warnings`, tests, shellcheck, overlay checksums.

## Device loop

The board is a Pi 5 at `rezor@192.168.1.9` with passwordless sudo and the repo in
`~/pt35-desktop`.

```sh
ssh rezor@192.168.1.9 'cd ~/pt35-desktop && git pull -q && \
  su -l rezor -c "cd ~/pt35-desktop && cargo build --release --workspace" && \
  sudo install -m755 target/release/pt35-menu /usr/bin/'
ssh rezor@192.168.1.9 'export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1; \
  pt35ctl menu toggle; grim /tmp/shot.png'
```

Config files go to `/usr/share/pt35-desktop/`. Binaries go to `/usr/bin/`.
`sudo systemctl restart greetd` restarts the whole session.

## Reference

`github.com/trailcurrentoss/TrailCurrentTracer` runs on the same hardware. Its
`docs/controls.md` and `tracerd/keymap.default.json` are the source of the button
map. Its design is the visual target: tile grid launcher, coloured button pills
in the hint bar, no battery gauge.
