#!/usr/bin/env bash
# pt35-desktop installer — a keyboard-driven desktop for the Waveshare PocketTerm35.
#
#   curl -fsSL https://raw.githubusercontent.com/therezor/PocketTerm35-Dersktop/main/install.sh | sudo bash
#
# or, from a checkout:   sudo ./install.sh
#
# Flags: --dry-run  --uninstall  --no-boot-config  --user NAME  --version  --help
set -euo pipefail

VERSION="0.1.0"
REPO="${PT35_REPO:-therezor/PocketTerm35-Dersktop}"
PREFIX=/usr
SHARE=$PREFIX/share/pt35-desktop
BOOTCFG=/boot/firmware/config.txt
MARK_BEGIN="# >>> pt35-desktop >>>"
MARK_END="# <<< pt35-desktop <<<"
DRY=0; UNINSTALL=0; SKIP_BOOT=0; TARGET_USER=""

msg()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[!]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[x]\033[0m %s\n' "$*" >&2; exit 1; }
run()  { if [ "$DRY" = 1 ]; then printf '   would run: %s\n' "$*"; else eval "$*"; fi; }

usage() { sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0; }

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --no-boot-config) SKIP_BOOT=1 ;;
    --user) TARGET_USER="${2:?--user needs a name}"; shift ;;
    --version) echo "pt35-desktop $VERSION"; exit 0 ;;
    -h|--help) usage ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

# ---------------------------------------------------------------- guard rails
[ "$(id -u)" = 0 ] || die "run as root (sudo $0)"

TARGET_USER="${TARGET_USER:-${SUDO_USER:-}}"
[ -n "$TARGET_USER" ] || die "cannot tell which user to set up; pass --user NAME"
id "$TARGET_USER" >/dev/null 2>&1 || die "no such user: $TARGET_USER"
USER_HOME=$(getent passwd "$TARGET_USER" | cut -d: -f6)

check_platform() {
  [ "$(dpkg --print-architecture)" = arm64 ] || die "arm64 (64-bit Raspberry Pi OS) required"
  local codename model
  codename=$(. /etc/os-release && echo "${VERSION_CODENAME:-unknown}")
  case "$codename" in
    trixie) ;;
    bookworm) die "Bookworm is not supported: its sway is not built against the Raspberry Pi wlroots. Use Raspberry Pi OS (Trixie) 64-bit." ;;
    *) warn "untested Debian release '$codename' — continuing, but expect breakage" ;;
  esac
  model=$(tr -d '\0' < /proc/device-tree/model 2>/dev/null || echo "")
  case "$model" in
    *"Raspberry Pi 5"*|*"Raspberry Pi 4"*) msg "board: $model" ;;
    *) warn "unrecognised board '$model' — PocketTerm35 ships with a Pi 4B or Pi 5" ;;
  esac
  if [ -f /usr/bin/labwc ] || systemctl list-unit-files 2>/dev/null | grep -q '^lightdm\.service'; then
    warn "this looks like Raspberry Pi OS Desktop; pt35-desktop will take over the session (the old one is restored by --uninstall)"
  fi
}

check_sway_build() {
  local cand
  cand=$(apt-cache policy sway 2>/dev/null | sed -n 's/^  Candidate: //p')
  [ -n "$cand" ] || die "no 'sway' package available — run 'apt update' first"
  case "$cand" in
    *rpt*) msg "sway candidate: $cand (Raspberry Pi build, good)" ;;
    *) warn "sway candidate '$cand' is not a Raspberry Pi (+rpt) build; it will probably fail against the patched libwlroots. Run 'apt update' and retry." ;;
  esac
}

# ---------------------------------------------------------------- source tree
# Either we sit in a checkout (config/ next to this script) or we fetch a release.
SRC=""
locate_source() {
  local here; here=$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || echo "")
  if [ -n "$here" ] && [ -d "$here/config" ]; then
    SRC="$here"; msg "installing from checkout: $SRC"; return
  fi
  local tmp; tmp=$(mktemp -d)
  msg "fetching pt35-desktop from github.com/$REPO"
  run "curl -fsSL 'https://codeload.github.com/$REPO/tar.gz/refs/heads/main' | tar -xz -C '$tmp' --strip-components=1"
  SRC="$tmp"
}

# ---------------------------------------------------------------- boot config
boot_config_block() {
  cat <<EOF
$MARK_BEGIN
# PocketTerm35: touchscreen (Goodix GT911 @0x5d) + USB host for the RP2040 keyboard.
# The '5b' overlay is correct on BOTH Pi 4B and Pi 5 — the two Waveshare overlays
# differ by touch I2C address, not by board.
dtparam=i2c_arm=on
dtoverlay=waveshare-35dpi-5b
dtoverlay=dwc2,dr_mode=host
$MARK_END
EOF
}

apply_boot_config() {
  [ "$SKIP_BOOT" = 1 ] && { msg "skipping boot config (--no-boot-config)"; return; }
  [ -f "$BOOTCFG" ] || BOOTCFG=/boot/config.txt
  [ -f "$BOOTCFG" ] || die "no config.txt found at /boot/firmware or /boot"

  local dtbo="$SRC/boot/waveshare-35dpi-5b.dtbo"
  if [ -f "$dtbo" ]; then
    if [ -f "$SRC/boot/SHA256SUMS" ]; then
      run "(cd '$SRC/boot' && sha256sum -c SHA256SUMS)"
    fi
    run "install -m644 '$dtbo' /boot/firmware/overlays/ 2>/dev/null || install -m644 '$dtbo' /boot/overlays/"
  else
    warn "no vendored DTBO in boot/ — get it from https://files.waveshare.com/wiki/common/3.5HDMI_E_DTBO.zip and drop it in boot/"
  fi

  if grep -qF "$MARK_BEGIN" "$BOOTCFG"; then
    msg "refreshing pt35 block in $BOOTCFG"
    run "sed -i '/$(printf '%s' "$MARK_BEGIN" | sed 's/[]\/$*.^[]/\\\\&/g')/,/$(printf '%s' "$MARK_END" | sed 's/[]\/$*.^[]/\\\\&/g')/d' '$BOOTCFG'"
  else
    run "cp -n '$BOOTCFG' '$BOOTCFG.pt35.bak'"
  fi
  if [ "$DRY" = 1 ]; then boot_config_block | sed 's/^/   would append: /'; else boot_config_block >> "$BOOTCFG"; fi
  msg "boot config updated ($BOOTCFG)"
}

# ---------------------------------------------------------------- packages
PKGS_CORE="sway swaybg xwayland foot seatd greetd keyd
           pipewire pipewire-alsa pipewire-pulse wireplumber
           xdg-desktop-portal-wlr wl-clipboard grim slurp
           fonts-dejavu-core fonts-noto-color-emoji
           i2c-tools evtest libinput-tools"
PKGS_APPS="helix imv zathura mpv"

install_packages() {
  msg "installing packages"
  run "DEBIAN_FRONTEND=noninteractive apt-get update"
  # shellcheck disable=SC2086
  run "DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends $PKGS_CORE"
  for p in $PKGS_APPS; do
    if apt-cache policy "$p" 2>/dev/null | grep -q 'Candidate: [^(]'; then
      run "DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends $p"
    else
      warn "package '$p' not in this release — skipped (ships in pt35-apps later)"
    fi
  done
}

# ---------------------------------------------------------------- files
install_files() {
  msg "installing configuration into $SHARE"
  run "install -d '$SHARE'"
  run "cp -r '$SRC/config/.' '$SHARE/'"
  run "install -d /etc/keyd"
  run "install -m644 '$SRC/config/keyd/pocketterm35.conf' /etc/keyd/pocketterm35.conf"

  # session launcher
  run "install -m755 '$SRC/scripts/pt35-session' $PREFIX/bin/pt35-session"
  run "install -m644 '$SRC/config/pt35-session.desktop' /usr/share/wayland-sessions/pt35-session.desktop"

  # per-user config: sway config that sources ours, and an empty override tree
  run "install -d -o '$TARGET_USER' -g '$TARGET_USER' '$USER_HOME/.config/sway' '$USER_HOME/.config/pt35' '$USER_HOME/.config/foot'"
  if [ ! -e "$USER_HOME/.config/sway/config" ]; then
    run "printf 'include %s/sway/config\\n\\n# your overrides below this line\\n' '$SHARE' > '$USER_HOME/.config/sway/config'"
    run "chown '$TARGET_USER:$TARGET_USER' '$USER_HOME/.config/sway/config'"
  else
    warn "$USER_HOME/.config/sway/config exists — left alone; add:  include $SHARE/sway/config"
  fi
  if [ ! -e "$USER_HOME/.config/foot/foot.ini" ]; then
    run "install -m644 -o '$TARGET_USER' -g '$TARGET_USER' '$SRC/config/foot/foot.ini' '$USER_HOME/.config/foot/foot.ini'"
  fi
}

configure_services() {
  msg "configuring greetd autologin for '$TARGET_USER'"
  run "install -d /etc/greetd"
  if [ -f /etc/greetd/config.toml ] && ! grep -q pt35-session /etc/greetd/config.toml; then
    run "cp -n /etc/greetd/config.toml /etc/greetd/config.toml.pt35.bak"
  fi
  if [ "$DRY" = 1 ]; then
    echo "   would write /etc/greetd/config.toml (autologin $TARGET_USER -> pt35-session)"
  else
    cat > /etc/greetd/config.toml <<EOF
[terminal]
vt = 1

[default_session]
command = "$PREFIX/bin/pt35-session"
user = "$TARGET_USER"
EOF
  fi
  run "usermod -aG input,video,render,audio '$TARGET_USER'"
  run "systemctl enable seatd.service keyd.service"
  run "systemctl disable lightdm.service 2>/dev/null || true"
  run "systemctl set-default graphical.target"
  run "systemctl enable greetd.service"
}

do_install() {
  check_platform
  check_sway_build
  locate_source
  install_packages
  install_files
  apply_boot_config
  configure_services
  cat <<EOF

  pt35-desktop $VERSION installed.

  Reboot to start it:   sudo reboot
  Uninstall:            sudo $0 --uninstall

EOF
}

do_uninstall() {
  msg "removing pt35-desktop"
  [ -f "$BOOTCFG" ] || BOOTCFG=/boot/config.txt
  if [ -f "$BOOTCFG.pt35.bak" ]; then
    run "mv '$BOOTCFG.pt35.bak' '$BOOTCFG'"
  else
    run "sed -i '/pt35-desktop >>>/,/pt35-desktop <<</d' '$BOOTCFG'"
  fi
  run "systemctl disable greetd.service 2>/dev/null || true"
  run "systemctl enable lightdm.service 2>/dev/null || true"
  [ -f /etc/greetd/config.toml.pt35.bak ] && run "mv /etc/greetd/config.toml.pt35.bak /etc/greetd/config.toml"
  run "rm -rf '$SHARE' $PREFIX/bin/pt35-session /usr/share/wayland-sessions/pt35-session.desktop /etc/keyd/pocketterm35.conf"
  msg "done — your ~/.config/{sway,pt35,foot} were left untouched. Reboot."
}

if [ "$UNINSTALL" = 1 ]; then do_uninstall; else do_install; fi
