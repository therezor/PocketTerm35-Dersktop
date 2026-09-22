# Running GUI apps on a 640×480 screen with no mouse

Filled in as applications are tested on the device. The question for each one is
not "does it start" but "can it be used with a thumb keyboard and a D-pad".

## The output-scale trick

`output * scale 0.75` gives clients an ~853×640 logical surface on the same
panel. That is usually enough for a GTK4 or Qt dialog that refuses to shrink
below ~600×500. It is **per output, not per app** — sway has no per-window
scale — so `pt35d` switches it when you move between workspaces, using the
`scale` in each app's `apps.toml` profile.

Sharpness depends on the toolkit:

| toolkit | at scale 0.75 |
|---|---|
| GTK4 ≥ 4.14, Qt 6 | crisp — they honour `fractional-scale-v1` (0.75 = 90/120) |
| Chromium | crisp with `--force-device-scale-factor` |
| GTK3, XWayland | soft — rendered at 853×640 and downscaled |

Known cost that cannot be avoided: GTK4/libadwaita draws its own headerbar and
the compositor cannot remove it (`gtk-decoration-layout=:` only strips the
buttons; `GTK_CSD=0` is GTK3-only). That is ~46 px of 480 gone in those apps.

## Test matrix

| app | toolkit | scale | pointer needed | verdict |
|---|---|---|---|---|
| `foot` | native | 1.0 | no | *(to test)* |
| `imv` | native | 1.0 | no | *(to test)* |
| `mpv` | native | 1.0 | no | *(to test)* |
| `zathura` | GTK3 | 0.75 | no | *(to test)* |
| `chromium` | own | 0.75 | yes | *(to test)* |
| a GTK4 app | GTK4 | 0.75 | yes | *(to test)* |
| a Qt app | Qt6 | 0.75 | yes | *(to test)* |
| an X11-only app | XWayland | 0.75 | yes | *(to test)* |

## When an app does not fit

1. Try `pt35ctl scale 0.6` (1067×800 logical) — smaller text, more room.
2. `Super`+`r` (`pt35ctl window fit`) drags an oversized window back on screen.
3. Arm the pointer (`Super`+`p`) and use grid jump (`g`) to reach controls that
   are only clickable.
4. If it is still unusable, record it here and suggest the terminal alternative.
