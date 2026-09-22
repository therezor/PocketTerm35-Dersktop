# PocketTerm35 hardware facts

Source of truth for everything the shell assumes about this device. Run
`scripts/pt35-probe.sh` on the unit and replace the **TODO** sections with its
output; the rest is already verified.

```sh
bash scripts/pt35-probe.sh > docs/hardware-facts.md 2>&1
```

Captured on a Pi 5 board, 2026-09-22.

## Verified

### Display — plain HDMI, no overlay needed
The 3.5" 640×480 IPS panel is driven over HDMI and comes up with the stock
`vc4-kms-v3d` driver. None of the three Waveshare overlays contains a display
node; they are touchscreen-only. Nothing in `config.txt` is needed for the
picture.

### Touchscreen — Goodix GT911 on I²C-1, address 0x5d
Decompiling the shipped overlays (see `boot/README.md`):

| overlay | GT911 nodes declared |
|---|---|
| `waveshare-35dpi-3b.dtbo` | `0x14` |
| `waveshare-35dpi-4b.dtbo` | `0x14` |
| `waveshare-35dpi-5b.dtbo` | `0x14` **and `0x5d`**, plus `irq-gpios` |

PocketTerm35 units answer at `0x5d`, so **`waveshare-35dpi-5b.dtbo` is the right
overlay on both the Pi 4B and the Pi 5** — the split is by I²C address, not by
board. Both nodes declare `touchscreen-size-x = 0x280` (640) and
`touchscreen-size-y = 0x1e0` (480).

### Keyboard — USB HID behind an RP2040
The keyboard, D-pad and gaming buttons hang off an on-board RP2040 that presents
itself over the Pi's USB OTG port, which is why `dtoverlay=dwc2,dr_mode=host` is
required. The same MCU drives brightness and volume, so those may never appear
in `/sys/class/backlight`.

`1209:0001` ("My Company My Custom Pico") declares five interfaces: CDC ACM
(`/dev/ttyACM0`, silent), one HID, and two audio. Linux sees:

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
the D-pad and the face buttons. Row 6 carries Fn, Ctrl, Alt, Select
(`PRINT_SCREEN`), Space, Start (`PAUSE`), right Alt and **Super** (`WINDOWS`),
so the unit does have a Super key.

### Boot configuration
```
dtparam=i2c_arm=on
dtoverlay=waveshare-35dpi-5b
dtoverlay=dwc2,dr_mode=host
```
On Trixie this file is `/boot/firmware/config.txt`, not `/boot/config.txt`.

### Software base
Raspberry Pi OS **Trixie** 64-bit. `sway` must be the Raspberry Pi rebuild
(`1.10.1-2+rpt1` or later) — the Debian build fails against the Pi's patched
`libwlroots-0.18`. `seatd` is required; Pi OS Lite ships no audio stack, so
PipeWire is installed by the installer.

## TODO — needs the device

| question | how to answer | why it matters |
|---|---|---|
| Exact keycode of every Fn combination | `sudo evtest /dev/input/event1` | `config/keyd/pocketterm35.conf` is guesswork until then |
| Which physical key could become Super | `sudo keyd monitor` | the sway config still needs Alt fallbacks without one |
| Is there a `/sys/class/backlight` device? | `ls /sys/class/backlight` | if not, brightness control is RP2040-only and `pt35ctl brightness` must say so |
| Is a battery gauge on I²C? | `sudo i2cdetect -y 1` (INA219 @0x41/0x43, MAX17048 @0x36) | decides whether the bar shows a battery at all |
| Where does audio come out? | `aplay -l`, `wpctl status` | HDMI audio through the panel board vs a separate DAC |
| Connector name and modes | `swaymsg -t get_outputs`, `/sys/class/drm/*/modes` | the sway config uses `output *` precisely because this is unknown |
| Touch orientation | touch each corner under `evtest` | may need a libinput calibration matrix |
| Does a GTK4 app render legibly at `output * scale 0.75`? | open one, look at it | this is the go/no-go for the whole GUI-app strategy |
