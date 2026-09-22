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
- Buttons: D-pad is arrows. A B X Y L R are the literal letters. Start is `KEY_PAUSE`,
  Select is `KEY_SYSRQ`.
- No `/sys/class/backlight` and no `/sys/class/power_supply`. Brightness and battery
  belong to the RP2040. Never show a battery gauge that reads `--`.

## Input model

Six of the twelve buttons type letters, so input is modal.

- Nav mode: A opens, B goes back, X searches, Y is the screen's second action,
  L/R page, D-pad moves, 1-9 pick a visible row.
- Filter mode: letters type. Start and Select still work, because they carry no
  character. Backspace on an empty filter returns to nav mode.
- Every screen shows the current legend in the hint bar.

## Layout

640x480 leaves no room for chrome that is not earning its place.

- Status bar 26px, menu header 44px, hint bar 44px, list rows 46px.
- Geometry lives in `config/pt35/theme.toml`. Do not hardcode sizes in a widget.

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
