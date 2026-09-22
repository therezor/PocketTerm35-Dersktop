#!/usr/bin/env bash
# pt35-probe.sh — Phase 0 hardware probe for the Waveshare PocketTerm35.
#
# Run ON THE DEVICE (Raspberry Pi inside the PocketTerm35), as your normal user:
#     bash scripts/pt35-probe.sh > docs/hardware-facts.md 2>&1
#
# It only reads; it changes nothing. Sections marked [MANUAL] need you to press keys.
set -uo pipefail

say()  { printf '\n## %s\n\n' "$*"; }
run()  { printf '```console\n$ %s\n' "$*"; eval "$*" 2>&1 | sed 's/^/ /;s/^ //' ; printf '```\n'; }
note() { printf '> %s\n\n' "$*"; }

printf '# PocketTerm35 hardware facts\n\n'
printf -- '- generated: %s\n' "$(date -Is)"
printf -- '- host: %s\n' "$(uname -a)"
printf -- '- model: %s\n\n' "$(tr -d '\0' < /proc/device-tree/model 2>/dev/null || echo unknown)"

say 'OS / release'
run 'cat /etc/os-release'
run 'dpkg --print-architecture'
run 'cat /proc/cpuinfo | tail -n 6'

say 'Boot configuration'
run 'cat /boot/firmware/config.txt 2>/dev/null || cat /boot/config.txt'
run 'ls /boot/firmware/overlays/ 2>/dev/null | grep -i -E "waveshare|goodix|dwc2" || echo "no matching overlays"'
run 'vcgencmd get_config int 2>/dev/null | head -n 20 || true'

say 'Display / DRM'
run 'ls -l /sys/class/drm/'
for c in /sys/class/drm/card*-*/; do
  [ -e "$c/modes" ] || continue
  run "echo '--- $c'; cat $c/status; head -n 5 $c/modes"
done
run 'command -v wlr-randr >/dev/null && wlr-randr || echo "wlr-randr not installed / no compositor running"'
run 'dmesg | grep -i -E "drm|hdmi|vc4" | tail -n 25'
note 'Wanted: confirm the native 640x480 mode, the connector name (HDMI-A-1 / HDMI-A-2) and whether hdmi_cvt/framebuffer_* lines are still required under KMS.'

say 'Backlight / LEDs'
run 'ls -l /sys/class/backlight/ 2>/dev/null || echo "NO /sys/class/backlight — brightness is RP2040-owned"'
run 'ls -l /sys/class/leds/ 2>/dev/null || true'

say 'I2C bus'
run 'command -v i2cdetect >/dev/null && sudo i2cdetect -y 1 || echo "install i2c-tools: sudo apt install i2c-tools"'
note 'Expect GT911 touch @ 0x5d. Look for a fuel gauge / current monitor: INA219 @0x41/0x43, MAX17048 @0x36. Nothing else found => battery status is LED-only.'

say 'Power supply / battery'
run 'ls -l /sys/class/power_supply/ 2>/dev/null || echo "none"'
for p in /sys/class/power_supply/*/; do run "echo '--- $p'; cat $p/uevent"; done
run 'vcgencmd pmic_read_adc 2>/dev/null | head -n 20 || true'
run 'vcgencmd get_throttled 2>/dev/null || true'

say 'Input devices'
run 'cat /proc/bus/input/devices'
run 'ls -l /dev/input/by-id/ /dev/input/by-path/ 2>/dev/null'
run 'lsusb'
note '[MANUAL] For every keyboard-ish event node run:  sudo evtest /dev/input/eventN'
note '[MANUAL] Press, one at a time, and record the emitted code: every Fn combination, each D-pad direction, each gaming button, the shoulder buttons, volume and brightness keys.'
note 'CRITICAL: if the D-pad/gaming buttons emit ABS_HAT0X / BTN_SOUTH (a HID gamepad) rather than KEY_UP / KEY_ENTER, sway and keyd cannot see them and pt35-pad (evdev->uinput) is required.'

say 'Touchscreen'
run 'dmesg | grep -i -E "goodix|gt911|touch" | tail -n 15'
run 'command -v libinput >/dev/null && sudo libinput list-devices | sed -n "1,80p" || echo "install libinput-tools"'
note '[MANUAL] Touch each screen corner in evtest and confirm axis orientation vs the panel; note any needed libinput calibration matrix.'

say 'Audio'
run 'aplay -l 2>&1 || true'
run 'cat /proc/asound/cards 2>/dev/null || true'
run 'command -v wpctl >/dev/null && wpctl status || echo "pipewire not installed"'
note 'Wanted: is the 2W speaker HDMI audio through the panel board, a separate DAC, or a USB device behind the RP2040?'

say 'Package availability (Trixie)'
for p in sway swaybg xwayland foot seatd greetd keyd pipewire wireplumber xdg-desktop-portal-wlr wl-clipboard grim slurp i2c-tools evtest libinput-tools helix yazi bottom bluetuith imv zathura mpv warpd ydotool; do
  printf -- '- `%s`: %s\n' "$p" "$(apt-cache policy "$p" 2>/dev/null | sed -n 's/^  Candidate: //p' | head -n1 || echo '(none)')"
done
printf '\n'
note 'sway MUST show an +rpt version (Raspberry Pi rebuild against their patched libwlroots-0.18) or it will not start.'
note 'Anything showing (none) has to ship as a static aarch64 binary in the pt35-apps .deb.'

say 'Memory baseline'
run 'free -m'
run 'systemd-analyze 2>/dev/null || true'
run 'systemctl list-units --type=service --state=running --no-pager'

printf '\n---\n\n*End of probe. Fill in the [MANUAL] sections by hand before committing.*\n'
