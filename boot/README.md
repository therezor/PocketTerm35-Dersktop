# Device-tree overlays

Vendored from Waveshare's `3.5HDMI_E_DTBO.zip`
(<https://files.waveshare.com/wiki/common/3.5HDMI_E_DTBO.zip>), unmodified.
`SHA256SUMS` pins exactly what was downloaded. A source install
(`install.sh --from-source`) checks it; the release package ships the `5b`
overlay as it was built.

## Which one to use

**`waveshare-35dpi-5b.dtbo`, on both the Pi 4B and the Pi 5.** The names suggest
a board split, but decompiling them shows the difference is the touch
controller's I²C address, not the SoC:

| overlay | GT911 nodes | notes |
|---|---|---|
| `waveshare-35dpi-3b.dtbo` | `0x14` | `brcm,bcm2708` |
| `waveshare-35dpi-4b.dtbo` | `0x14` | `brcm,bcm2708` |
| `waveshare-35dpi-5b.dtbo` | `0x14` **and `0x5d`** | `brcm,bcm2835`, adds `irq-gpios` |

PocketTerm35 units answer at `0x5d`, so the `4b` overlay leaves the driver
probing an address with nothing on it and the touchscreen fails with I/O errors.
This matches what the community Kali port found on hardware.

All three overlays contain **only** the touchscreen node. There is no display
node, because the panel is driven over plain HDMI at 640×480 and needs no
overlay at all. All three declare a 640×480 touch area (`touchscreen-size-x = 0x280`,
`touchscreen-size-y = 0x1e0`).

## Refreshing them

```sh
curl -fsSLO https://files.waveshare.com/wiki/common/3.5HDMI_E_DTBO.zip
unzip -j 3.5HDMI_E_DTBO.zip -d .
sha256sum *.dtbo > SHA256SUMS
```
