# Ideas taken from TrailCurrent Tracer, and what is left

`github.com/trailcurrentoss/TrailCurrentTracer` runs on the same board. Its
docs and code were read for this project. What was taken, and what is still open.

## Taken

- **Button map.** Their `tracerd/keymap.default.json` was captured from hardware
  twice. It matches our probe of the same USB device (`1209:0001`): D-pad is
  arrows, A B X Y L R are literal letters, Start is `KEY_PAUSE`, Select is
  `KEY_SYSRQ`.
- **Modal input.** Six buttons type, so nav mode and filter mode are separate.
  Start and Select carry no character, so they work in both.
- **Hint bar.** A legend of the current button meanings, on every screen, with
  the pill coloured like the physical button. It is also a touch target.
- **Tile grid launcher.** 3 across, tinted badge, label and live second line.
- **Touch as the recovery path.** The RP2040 can strand itself in BOOTSEL and
  take every button with it. The GT911 is on I2C and keeps working.
- **No battery gauge.** Nothing on this board exposes charge. A gauge stuck at
  `--` reads as a flat battery, which is worse than no gauge.
- **Power button opens a menu.** `HandlePowerKey=ignore`, long press still
  powers off.

## Open

- **Toasts.** Short confirmation of an action that has no visible result
  ("Screenshot saved").
- **Spinner for slow lists.** A Wi-Fi scan takes seconds; an empty list looks
  broken.
- **"Daemon offline" screen.** The bar says `pt35d?` today. A full screen state
  is harder to miss.
- **Per-screen hint legends.** Our legend is the same everywhere. Their `HINTS`
  table gives each screen its own.
- **Button remapping in Settings**, seeded from a capture tool, for units whose
  firmware differs.
- **Hide the cursor by default.** They ship a udev rule that stops the HID mouse
  creating a pointer. We hide it after 1.5s of no movement instead.
- **Boot splash.** Straight from firmware to the shell with a logo, no console
  text.
- **Wi-Fi gate at boot.** A full screen picker when there is no usable network.
