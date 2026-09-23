# PocketTerm35 hardware facts

What the shell assumes about this device, checked on the board. To check a
unit of your own, run the probe and compare:

```sh
sudo /usr/share/pt35-desktop/pt35-probe.sh > /tmp/probe.md 2>&1
```

Captured on a Pi 5 board, 2026-09-22.

## Verified

### Display: plain HDMI, no overlay needed
The 3.5" 640×480 IPS panel is driven over HDMI and comes up with the stock
`vc4-kms-v3d` driver. None of the three Waveshare overlays contains a display
node; they are touchscreen-only. Nothing in `config.txt` is needed for the
picture.

### Touchscreen: Goodix GT911 on I²C-1, address 0x5d
Decompiling the shipped overlays (see `boot/README.md`):

| overlay | GT911 nodes declared |
|---|---|
| `waveshare-35dpi-3b.dtbo` | `0x14` |
| `waveshare-35dpi-4b.dtbo` | `0x14` |
| `waveshare-35dpi-5b.dtbo` | `0x14` **and `0x5d`**, plus `irq-gpios` |

PocketTerm35 units answer at `0x5d`, so **`waveshare-35dpi-5b.dtbo` is the right
overlay on both the Pi 4B and the Pi 5**. The split is by I²C address, not by
board. Both nodes declare `touchscreen-size-x = 0x280` (640) and
`touchscreen-size-y = 0x1e0` (480).

### Keyboard: USB HID behind an RP2040
The keyboard, D-pad and gaming buttons hang off an on-board RP2040 that presents
itself over the Pi's USB OTG port, which is why `dtoverlay=dwc2,dr_mode=host` is
required. The same MCU drives the backlight (a PWM pin, GP20) and the speaker
gain, so there is no `/sys/class/backlight`.

`1209:0001` ("My Company My Custom Pico") declares five interfaces: CDC ACM
(`/dev/ttyACM0`), one HID, and two audio. The ACM port is the CircuitPython
console: `pt35-kbd` flashes through it, and the patched firmware takes
brightness commands on it (below). Linux sees:

```
event1  My Company My Custom Pico Keyboard   sysrq kbd leds
event3  My Company My Custom Pico Mouse      5 buttons
```

**There is no gamepad.** Everything with a key on it arrives on `event1`: the
67-key QWERTY, the D-pad and the face buttons, one device, one keymap. Stock,
the A button and the `a` key send the same `KEY_A`, so no remapper (keyd
included) can separate them: keyd binds per device, and this is one device.

The firmware is **CircuitPython 10.0.0-beta.0** with `boot.py` calling
`storage.disable_usb_drive()`, which is why no CIRCUITPY drive appears. `code.py`
holds the matrix as a plain table, and `firmware/` changes the face-button row to
F13-F18. See `firmware/README.md`.

Matrix: rows `GP16 GP10 GP11 GP12 GP13 GP14 GP15`, columns `GP0`-`GP9`. Row 0 is
the D-pad and the face buttons. Row 6 carries Fn, Ctrl, Alt, Select, Space,
Start, right Alt and **Super** (`WINDOWS`), so the unit does have a Super key.
Stock, Select is `PRINT_SCREEN` and Start is `PAUSE`. The patched firmware
sends F21 and F22 for them and keeps Print and Pause on Fn.

### Network: Ethernet and Wi-Fi
The Pi's own `eth0` (RJ45 on the board) and `wlan0`, both under
NetworkManager. With no cable `eth0` is `down` and nmcli calls it
`unavailable`. `/proc/net/wireless` lists only the radio, so a wired link has
no signal reading. A cable with link carries the traffic when both are up.

### Boot configuration
```
dtparam=i2c_arm=on
dtoverlay=waveshare-35dpi-5b
dtoverlay=dwc2,dr_mode=host
```
On Trixie this file is `/boot/firmware/config.txt`, not `/boot/config.txt`.

### Software base
Raspberry Pi OS **Trixie** 64-bit. `sway` must be the Raspberry Pi rebuild
(`1.10.1-2+rpt1` or later): the Debian build fails against the Pi's patched
`libwlroots-0.18`. `seatd` is required. Pi OS Lite ships no audio stack, so
the package recommends PipeWire and `install.sh` installs it.

### Brightness over serial
The patched firmware reads `B<0-100>` and `B?` lines on `/dev/ttyACM0` and
answers `PT35 B=<n>`, with a 5% floor. `pt35d` drives it, and the answer is also
how the shell knows the patched firmware is on. Stock firmware never answers:
brightness is then Fn with `-` and `=` only.

### Battery
There is no `/sys/class/power_supply` and no gauge on I²C. The bar shows no
battery.

### Still open
- Where audio comes out: HDMI through the panel board, or a separate DAC.
  Volume works through PipeWire either way.
