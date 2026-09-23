# Ideas from TrailCurrent Tracer, and what is left

`github.com/trailcurrentoss/TrailCurrentTracer` runs on the same board. Its
docs and code were read for this project.

## Taken

- **Button map.** Their `tracerd/keymap.default.json` matches our probe of the
  same USB device (`1209:0001`): the D-pad is arrows and, on stock firmware,
  A B X Y L R are letters, Start is `KEY_PAUSE`, Select is `KEY_SYSRQ`.
- **Modal menu.** On stock firmware six buttons type, so the menu has a nav mode
  and a filter mode. Start and Select carry no character, so they work in both.
- **Hint bar.** The current button meanings on every screen, each pill coloured
  like its button. It is also a touch target.
- **Tile grid.** 3 across, tinted badge, label and a live second line.
- **Touch as the way back in.** The RP2040 can get stuck and take every button
  with it. The GT911 touch panel is on I2C and keeps working.
- **No battery gauge.** Nothing on this board reports charge. A gauge stuck at
  `--` reads as a flat battery.
- **Power button opens a menu.** `HandlePowerKey=ignore`. A long press still
  powers off.
- **Toasts.** A message takes the taskbar for a couple of seconds. It is the
  only answer a key binding can give.
- **Placeholder for slow lists.** Wi-Fi and Bluetooth show "Scanning..." and
  fill in from a worker thread, so the menu never freezes.
- **Recents.** The last five launches sit at the top of the launcher.

## Open

- **"Daemon offline" screen.** Today the bar says `pt35d?` and an empty menu
  screen says "pt35d is not answering". A full screen would be harder to miss.
- **Button remapping in Settings,** for units whose firmware differs.
- **Boot splash.** From firmware to the shell with a logo, no console text.
- **Wi-Fi setup at boot.** A full-screen picker when there is no network.
