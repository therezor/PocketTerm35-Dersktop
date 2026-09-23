# Keyboard firmware

The PocketTerm35's keyboard is an RP2040 running CircuitPython 10. Everything
with a key on it, the 67-key QWERTY, the D-pad and the face buttons, is one
matrix on one HID device, `event1`.

Stock, the face buttons send the letters `l r x y b a`: the same keycodes the
QWERTY sends. Nothing above the firmware can tell button A from key `a`, so
either the buttons work as buttons and you cannot type those letters, or the
other way round. keyd cannot help: it binds per device, and this is one device.

`code.py` here changes the keymap so the buttons send keys of their own. The
shell then binds them everywhere and the letters keep typing. It also adds
brightness control over the USB console.

| button | keycode | keysym under the default layout |
|---|---|---|
| L | F13 | `XF86Tools` |
| R | F14 | `XF86Launch5` |
| X | F15 | `XF86Launch6` |
| Y | F16 | `XF86Launch7` |
| B | F17 | `XF86Launch8` |
| A | F18 | `XF86Launch9` |
| Select | F21 | `XF86TouchpadToggle` |
| Start | F22 | `XF86TouchpadOn` |

`inet(evdev)` claims those keycodes before `F13`..`F22` get a chance, which is
why the names look odd. Fn+Select is still Print Screen (a screenshot) and
Fn+Start still Pause. The D-pad (arrows) and Super are unchanged.

## Brightness

The backlight is a PWM pin on the RP2040. The patched firmware reads `B<0-100>`
and `B?` on `/dev/ttyACM0` and answers `PT35 B=<n>`. `pt35d` uses it for the
brightness slider, and to tell that this firmware is on.

## Flashing

`boot.py` calls `storage.disable_usb_drive()`, so there is no CIRCUITPY drive.
The way in is the REPL on `/dev/ttyACM0`:

```sh
sudo pt35-kbd flash                # the patched keymap
sudo pt35-kbd restore              # the stock one
sudo pt35-kbd dump code.py > /tmp/backup.py
sudo pt35-kbd files
```

`install.sh --flash-keyboard` does the same as `flash`.

Run it on the Pi, not over the network: the serial port is local to the device.
`python3-serial` is the only dependency.

## Going back

`stock/code.py` is the original, read off the board before the change.

```sh
sudo pt35-kbd restore
```

A truncated or broken `code.py` cannot brick the board: CircuitPython drops to
the REPL on this same port, so the file can always be rewritten. Double-tapping
RESET enters safe mode, which skips `boot.py` and brings the USB drive back.
