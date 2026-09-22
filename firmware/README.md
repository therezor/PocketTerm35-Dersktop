# Keyboard firmware

The PocketTerm35's keyboard is an RP2040 running CircuitPython 10. Everything
with a key on it, the 67-key QWERTY, the D-pad and the face buttons, is one
matrix on one HID device, `event1`.

Stock, the face buttons send the letters `l r x y b a`: the same keycodes the
QWERTY sends. Nothing above the firmware can tell button A from key `a`, so
either the buttons work as buttons and you cannot type those letters, or the
other way round. keyd cannot help: it binds per device, and this is one device.

`code.py` here changes the keymap so the six send F13-F18 instead. The shell
then binds them everywhere and the letters keep typing.

| button | keycode | keysym under the default layout |
|---|---|---|
| L | F13 | `XF86Tools` |
| R | F14 | `XF86Launch5` |
| X | F15 | `XF86Launch6` |
| Y | F16 | `XF86Launch7` |
| B | F17 | `XF86Launch8` |
| A | F18 | `XF86Launch9` |

`inet(evdev)` claims those keycodes before `F13`..`F18` get a chance, which is
why the names look odd. D-pad (arrows), Start (`KEY_PAUSE`), Select
(`KEY_SYSRQ`) and Super are already unique and are untouched.

## Flashing

`boot.py` calls `storage.disable_usb_drive()`, so there is no CIRCUITPY drive.
The way in is the REPL on `/dev/ttyACM0`:

```sh
scripts/pt35-kbd write code.py firmware/code.py   # writes, then reads back to verify
scripts/pt35-kbd dump code.py > /tmp/backup.py    # what is on it now
scripts/pt35-kbd files
```

Run it on the Pi, not over the network: the serial port is local to the device.
`python3-serial` is the only dependency.

## Going back

`stock/code.py` is the original, read off the board before the change.

```sh
scripts/pt35-kbd write code.py firmware/stock/code.py
```

A truncated or broken `code.py` cannot brick the board: CircuitPython drops to
the REPL on this same port, so the file can always be rewritten. Double-tapping
RESET enters safe mode, which skips `boot.py` and brings the USB drive back.
