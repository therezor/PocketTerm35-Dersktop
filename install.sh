#!/usr/bin/env bash
# pt35-desktop — keyboard-driven desktop for the Waveshare PocketTerm35.
#
#   curl -fsSL https://raw.githubusercontent.com/therezor/PocketTerm35-Dersktop/main/install.sh | sudo bash
#
# or, from a checkout:   sudo ./install.sh
#
# Flags:
#   --dry-run          print what would happen, change nothing
#   --uninstall        remove pt35-desktop and restore the previous session
#   --from-source      build the binaries here instead of downloading a release
#   --user NAME        set up the session for this user (default: $SUDO_USER)
#   --version          print the installer version
set -euo pipefail

VERSION="0.1.0"
REPO="${PT35_REPO:-therezor/PocketTerm35-Dersktop}"
SHARE=/usr/share/pt35-desktop
DRY=0; UNINSTALL=0; FROM_SOURCE=0; TARGET_USER=""

msg()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[!]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[x]\033[0m %s\n' "$*" >&2; exit 1; }
run()  { if [ "$DRY" = 1 ]; then printf '    would run: %s\n' "$*"; else eval "$*"; fi; }

usage() { sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0; }

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --from-source) FROM_SOURCE=1 ;;
    --user) TARGET_USER="${2:?--user needs a name}"; shift ;;
    --version) echo "pt35-desktop installer $VERSION"; exit 0 ;;
    -h|--help) usage ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

[ "$(id -u)" = 0 ] || die "run as root (sudo $0)"

TARGET_USER="${TARGET_USER:-${SUDO_USER:-}}"
if [ -z "$TARGET_USER" ] || [ "$TARGET_USER" = root ]; then
  TARGET_USER=$(getent passwd | awk -F: '$3 >= 1000 && $3 < 60000 && $7 !~ /nologin|false/ { print $1; exit }')
fi
[ -n "$TARGET_USER" ] || die "cannot tell which user to set up; pass --user NAME"
id "$TARGET_USER" >/dev/null 2>&1 || die "no such user: $TARGET_USER"

# ---------------------------------------------------------------- guard rails
check_platform() {
  [ "$(dpkg --print-architecture)" = arm64 ] || die "arm64 (64-bit Raspberry Pi OS) is required"

  local codename model
  codename=$(. /etc/os-release && echo "${VERSION_CODENAME:-unknown}")
  case "$codename" in
    trixie) ;;
    bookworm)
      die "Bookworm is not supported: its sway is not built against the Raspberry Pi wlroots.
     Use Raspberry Pi OS (Trixie) 64-bit — Lite is the intended base." ;;
    *) warn "untested Debian release '$codename'; continuing, but expect breakage" ;;
  esac

  model=$(tr -d '\0' < /proc/device-tree/model 2>/dev/null || echo "")
  case "$model" in
    *"Raspberry Pi 5"*|*"Raspberry Pi 4"*) msg "board: $model" ;;
    *) warn "unrecognised board '${model:-unknown}'; the PocketTerm35 ships with a Pi 4B or Pi 5" ;;
  esac

  if [ -x /usr/bin/labwc ] || systemctl list-unit-files 2>/dev/null | grep -q '^lightdm\.service'; then
    warn "this looks like Raspberry Pi OS Desktop: pt35-desktop will take over the session (--uninstall restores it)"
  fi
}

check_sway_build() {
  run "DEBIAN_FRONTEND=noninteractive apt-get update -qq"
  local candidate
  candidate=$(apt-cache policy sway 2>/dev/null | sed -n 's/^  Candidate: //p')
  [ -n "$candidate" ] && [ "$candidate" != "(none)" ] || die "no 'sway' package available"
  case "$candidate" in
    *rpt*) msg "sway candidate $candidate (Raspberry Pi build)" ;;
    *) warn "sway candidate '$candidate' is not a Raspberry Pi (+rpt) build; it will likely fail against the patched libwlroots" ;;
  esac
}

# ------------------------------------------------------------------- install
source_dir() {
  local here
  here=$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || echo "")
  if [ -n "$here" ] && [ -d "$here/config" ] && [ -d "$here/crates" ]; then
    echo "$here"
  fi
}

install_from_release() {
  local tmp url deb sums
  tmp=$(mktemp -d)
  url="https://github.com/$REPO/releases/latest/download"
  deb="$tmp/pt35-desktop_arm64.deb"
  sums="$tmp/SHA256SUMS"

  msg "downloading the latest release from $REPO"
  run "curl -fsSL -o '$deb' '$url/pt35-desktop_arm64.deb'"
  if curl -fsSL -o "$sums" "$url/SHA256SUMS" 2>/dev/null; then
    msg "verifying checksum"
    run "(cd '$tmp' && grep 'pt35-desktop_arm64.deb' SHA256SUMS | sha256sum -c -)"
  else
    warn "no SHA256SUMS published for this release; skipping checksum verification"
  fi
  run "PT35_USER='$TARGET_USER' DEBIAN_FRONTEND=noninteractive apt-get install -y '$deb'"
}

install_from_source() {
  local src="$1"
  command -v cargo >/dev/null || die "cargo is not installed; use the release path or install rustup first"
  msg "building (this takes a while on a Pi)"
  run "su '$TARGET_USER' -c 'cd \"$src\" && cargo build --release --workspace'"

  msg "installing dependencies"
  run "DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        sway xwayland foot seatd greetd keyd \
        pipewire pipewire-alsa pipewire-pulse wireplumber \
        xdg-desktop-portal-wlr wl-clipboard grim \
        fonts-dejavu-core i2c-tools evtest"

  msg "installing files"
  for binary in pt35d pt35ctl pt35-bar pt35-menu pt35-pointer; do
    run "install -m755 '$src/target/release/$binary' /usr/bin/$binary"
  done
  run "install -m755 '$src/scripts/pt35-session' /usr/bin/pt35-session"
  run "install -d /usr/lib/pt35 && install -m755 '$src/scripts/pt35-cpu-profile' /usr/lib/pt35/pt35-cpu-profile"
  run "install -d '$SHARE/sway' '$SHARE/pt35' '$SHARE/foot' '$SHARE/boot'"
  run "install -m644 '$src/config/sway/config' '$SHARE/sway/config'"
  run "install -m644 '$src/config/foot/foot.ini' '$SHARE/foot/foot.ini'"
  run "install -m644 '$src/config/pt35/'*.toml '$SHARE/pt35/'"
  run "install -m755 '$src/scripts/pt35-probe.sh' '$SHARE/pt35-probe.sh'"
  run "install -d /etc/keyd && install -m644 '$src/config/keyd/pocketterm35.conf' /etc/keyd/"
  run "install -m440 '$src/packaging/pt35-cpu-profile.sudoers' /etc/sudoers.d/pt35-cpu-profile"
  run "install -d /usr/share/wayland-sessions && install -m644 '$src/config/pt35-session.desktop' /usr/share/wayland-sessions/"
  if [ -f "$src/boot/waveshare-35dpi-5b.dtbo" ]; then
    [ -f "$src/boot/SHA256SUMS" ] && run "(cd '$src/boot' && sha256sum -c SHA256SUMS)"
    run "install -m644 '$src/boot/waveshare-35dpi-5b.dtbo' '$SHARE/boot/'"
  else
    warn "no device-tree overlay in boot/ — the touchscreen will not work.
     Fetch it: see boot/README.md"
  fi

  msg "wiring up the session"
  run "PT35_USER='$TARGET_USER' '$src/packaging/debian/postinst' configure"
}

do_install() {
  check_platform
  check_sway_build

  local src
  src=$(source_dir)
  if [ "$FROM_SOURCE" = 1 ] || [ -n "$src" ]; then
    [ -n "$src" ] || die "--from-source needs a checkout; clone $REPO first"
    msg "installing from the checkout at $src"
    install_from_source "$src"
  else
    install_from_release
  fi

  cat <<EOF

  pt35-desktop $VERSION is installed for '$TARGET_USER'.

  Reboot to start it:   sudo reboot
  Uninstall:            sudo $0 --uninstall

EOF
}

do_uninstall() {
  if dpkg -s pt35-desktop >/dev/null 2>&1; then
    msg "removing the pt35-desktop package"
    run "DEBIAN_FRONTEND=noninteractive apt-get remove -y pt35-desktop"
    return
  fi
  msg "removing pt35-desktop (installed from source)"
  if [ -x "$SHARE/../../lib/pt35/pt35-cpu-profile" ] || [ -d "$SHARE" ]; then
    local src; src=$(source_dir)
    if [ -n "$src" ] && [ -x "$src/packaging/debian/prerm" ]; then
      run "'$src/packaging/debian/prerm' remove"
    fi
  fi
  run "rm -f /usr/bin/pt35d /usr/bin/pt35ctl /usr/bin/pt35-bar /usr/bin/pt35-menu \
        /usr/bin/pt35-pointer /usr/bin/pt35-session /etc/keyd/pocketterm35.conf \
        /etc/sudoers.d/pt35-cpu-profile /usr/share/wayland-sessions/pt35-session.desktop"
  run "rm -rf '$SHARE' /usr/lib/pt35"
  msg "done — your ~/.config/{sway,pt35,foot} were left untouched. Reboot."
}

if [ "$UNINSTALL" = 1 ]; then do_uninstall; else do_install; fi
