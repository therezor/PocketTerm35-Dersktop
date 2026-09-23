#!/usr/bin/env bash
# pt35-desktop: a pocket desktop for the Waveshare PocketTerm35.
#
#   curl -fsSL https://raw.githubusercontent.com/therezor/PocketTerm35-Dersktop/main/install.sh | sudo bash
#
# or, from a checkout:   sudo ./install.sh
#
# Flags:
#   --dry-run          print what would happen, change nothing
#   --uninstall        remove pt35-desktop and restore the previous session
#   --from-source      build the binaries here instead of downloading a release
#   --flash-keyboard   also put the face buttons on F13-F18 (see firmware/)
#   --user NAME        set up the session for this user (default: $SUDO_USER)
#   --version          print the installer version
set -euo pipefail

VERSION="0.1.0"
REPO="${PT35_REPO:-therezor/PocketTerm35-Dersktop}"
SHARE=/usr/share/pt35-desktop
DRY=0; UNINSTALL=0; FROM_SOURCE=0; FLASH_KEYBOARD=0; TARGET_USER=""

msg()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[!]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[x]\033[0m %s\n' "$*" >&2; exit 1; }
run()  { if [ "$DRY" = 1 ]; then printf '    would run: %s\n' "$*"; else eval "$*"; fi; }

# The help is a heredoc, not a read of "$0": through `curl | bash` there is no
# script file to read.
usage() {
  cat <<'HELP'
pt35-desktop installer

  curl -fsSL https://raw.githubusercontent.com/therezor/PocketTerm35-Dersktop/main/install.sh | sudo bash
  curl -fsSL .../install.sh | sudo bash -s -- --uninstall
  sudo ./install.sh [flags]            from a checkout

Flags:
  --dry-run          print what would happen, change nothing
  --uninstall        remove pt35-desktop and restore the previous session
  --from-source      build the binaries here instead of downloading a release
  --flash-keyboard   also flash the keyboard firmware (see firmware/)
  --user NAME        set up the session for this user (default: $SUDO_USER)
  --version          print the installer version
HELP
  exit 0
}

# How to run this installer again, the way it was run this time.
again() {
  if [ -n "$(source_dir)" ]; then
    echo "sudo ./install.sh $*"
  else
    echo "curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | sudo bash -s -- $*"
  fi
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --from-source) FROM_SOURCE=1 ;;
    --flash-keyboard) FLASH_KEYBOARD=1 ;;
    --user) TARGET_USER="${2:?--user needs a name}"; shift ;;
    --version) echo "pt35-desktop installer $VERSION"; exit 0 ;;
    -h|--help) usage ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

[ "$(id -u)" = 0 ] || die "run as root: $(again)"

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
     Use Raspberry Pi OS (Trixie) 64-bit. Lite is the intended base." ;;
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
  if ! curl -fsSL -o "$deb" "$url/pt35-desktop_arm64.deb"; then
    warn "no published release yet: building from source instead"
    clone_and_build "$tmp"
    return
  fi
  if curl -fsSL -o "$sums" "$url/SHA256SUMS" 2>/dev/null; then
    msg "verifying checksum"
    run "(cd '$tmp' && grep 'pt35-desktop_arm64.deb' SHA256SUMS | sha256sum -c -)"
  else
    warn "no SHA256SUMS published for this release; skipping checksum verification"
  fi
  run "PT35_USER='$TARGET_USER' DEBIAN_FRONTEND=noninteractive apt-get install -y '$deb'"
}

# No release to download: fetch the source and build it here instead, so the
# one-command install works even before the first tag is cut.
clone_and_build() {
  local tmp="$1"
  run "DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends git"
  run "git clone --depth 1 'https://github.com/$REPO' '$tmp/src'"
  run "chown -R '$TARGET_USER' '$tmp/src'"
  install_from_source "$tmp/src"
}

install_from_source() {
  local src="$1"

  # Runtime packages first, then the build dependencies: smithay-client-toolkit
  # needs libxkbcommon through pkg-config, so building before this fails.
  msg "installing dependencies"
  run "DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        sway xwayland foot seatd greetd keyd \
        pipewire pipewire-alsa pipewire-pulse wireplumber \
        xdg-desktop-portal-wlr wl-clipboard grim wtype \
        fonts-dejavu-core papirus-icon-theme i2c-tools evtest python3-serial htop \
        qt5-gtk-platformtheme qt6-gtk-platformtheme fastfetch bibata-cursor-theme ncdu \
        build-essential pkg-config libxkbcommon-dev"

  # A login shell, so a rustup toolchain in ~/.cargo/bin wins over the older
  # /usr/bin/cargo that Debian ships (Trixie's 1.85 is too old for wayland-protocols).
  su -l "$TARGET_USER" -c 'command -v cargo >/dev/null' \
    || die "cargo is not installed for '$TARGET_USER'; install rustup, or drop --from-source to use a release build"
  msg "cargo: $(su -l "$TARGET_USER" -c 'cargo --version')"
  msg "building (10-40 minutes on a Pi 4)"
  run "su -l '$TARGET_USER' -c 'cd \"$src\" && cargo build --release --workspace'"

  msg "installing files"
  for binary in pt35d pt35ctl pt35-bar pt35-menu; do
    run "install -m755 '$src/target/release/$binary' /usr/bin/$binary"
  done
  run "install -m755 '$src/scripts/pt35-session' /usr/bin/pt35-session"
  run "install -m755 '$src/scripts/pt35-kbd' /usr/bin/pt35-kbd"
  run "install -m755 '$src/scripts/pt35-quick' /usr/bin/pt35-quick"
  run "install -m755 '$src/scripts/pt35-hello' /usr/bin/pt35-hello"
  run "install -d /usr/lib/pt35 && install -m755 '$src/scripts/pt35-cpu-profile' /usr/lib/pt35/pt35-cpu-profile"
  run "install -d '$SHARE/sway' '$SHARE/pt35' '$SHARE/foot' '$SHARE/boot' '$SHARE/fastfetch'"
  run "install -m644 '$src/config/fastfetch/skull.txt' '$SHARE/fastfetch/'"
  run "install -d /etc/xdg/fastfetch && install -m644 '$src/config/fastfetch/config.jsonc' /etc/xdg/fastfetch/"
  run "install -m644 '$src/config/sway/config' '$SHARE/sway/config'"
  run "install -m644 '$src/config/foot/foot.ini' '$SHARE/foot/foot.ini'"
  run "install -m644 '$src/config/pt35/'*.toml '$SHARE/pt35/'"
  run "install -m755 '$src/scripts/pt35-probe.sh' '$SHARE/pt35-probe.sh'"
  run "install -d '$SHARE/firmware/stock'"
  run "install -m644 '$src/firmware/code.py' '$SHARE/firmware/'"
  run "install -m644 '$src/firmware/README.md' '$SHARE/firmware/'"
  run "install -m644 '$src/firmware/stock/code.py' '$SHARE/firmware/stock/'"
  run "install -d '$SHARE/logind' && install -m644 '$src/config/logind/pt35.conf' '$SHARE/logind/'"
  run "install -d '$SHARE/systemd' && install -m644 '$src/config/systemd/greetd-vt1.conf' '$SHARE/systemd/'"
  run "install -d /etc/keyd && install -m644 '$src/config/keyd/pocketterm35.conf' /etc/keyd/"
  run "install -m440 '$src/packaging/pt35-cpu-profile.sudoers' /etc/sudoers.d/pt35-cpu-profile"
  run "install -m644 '$src/packaging/70-pt35-uinput.rules' /etc/udev/rules.d/70-pt35-uinput.rules"
  run "install -d /usr/share/wayland-sessions && install -m644 '$src/config/pt35-session.desktop' /usr/share/wayland-sessions/"
  if [ -f "$src/boot/waveshare-35dpi-5b.dtbo" ]; then
    [ -f "$src/boot/SHA256SUMS" ] && run "(cd '$src/boot' && sha256sum -c SHA256SUMS)"
    run "install -m644 '$src/boot/waveshare-35dpi-5b.dtbo' '$SHARE/boot/'"
  else
    warn "no device-tree overlay in boot/: the touchscreen will not work.
     Fetch it: see boot/README.md"
  fi

  msg "wiring up the session"
  run "PT35_USER='$TARGET_USER' '$src/packaging/debian/postinst' configure"
}

# Yazi is the file manager, and Trixie does not package it. Upstream's static
# musl build runs on any arm64 Pi. Its icons are Nerd Font glyphs, also not
# packaged: the symbols-only font is enough, foot finds it as a fallback.
install_yazi() {
  local tmp url fonts=/usr/local/share/fonts/nerd-symbols
  tmp=$(mktemp -d)
  run "DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends unzip file fontconfig"
  if [ ! -d "$fonts" ]; then
    msg "installing the Nerd Font symbols"
    # Unpacked aside and moved into place, so a failure leaves no empty folder
    # that would stop the next run from trying again.
    if run "curl -fsSL -o '$tmp/symbols.zip' https://github.com/ryanoasis/nerd-fonts/releases/latest/download/NerdFontsSymbolsOnly.zip" \
      && run "unzip -q -o '$tmp/symbols.zip' '*.ttf' -d '$tmp/fonts'"; then
      run "install -d '$fonts' && install -m644 '$tmp/fonts/'*.ttf '$fonts/' && fc-cache -f '$fonts'"
    else
      warn "could not install the Nerd Font symbols; yazi will show boxes for icons"
    fi
  fi
  command -v yazi >/dev/null && { msg "yazi: $(yazi --version | head -n1)"; return; }
  url="https://github.com/sxyazi/yazi/releases/latest/download/yazi-aarch64-unknown-linux-musl.zip"
  msg "installing yazi"
  if ! run "curl -fsSL -o '$tmp/yazi.zip' '$url'"; then
    warn "could not download yazi; Files will not open until it is installed"
    return
  fi
  run "unzip -q -o '$tmp/yazi.zip' -d '$tmp'"
  run "install -m755 '$tmp/yazi-aarch64-unknown-linux-musl/yazi' '$tmp/yazi-aarch64-unknown-linux-musl/ya' /usr/local/bin/"
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
  install_yazi

  if [ "$FLASH_KEYBOARD" = 1 ]; then
    msg "putting the face buttons on F13-F18"
    run "pt35-kbd flash"
  fi

  cat <<EOF

  pt35-desktop $VERSION is installed for '$TARGET_USER'.

  Reboot to start it:   sudo reboot
  Uninstall:            $(again --uninstall)
EOF
  [ "$FLASH_KEYBOARD" = 1 ] || cat <<EOF

  A B X Y L R still send the letters a b x y l r, the same keycodes the keyboard
  sends, so the shell cannot use them as buttons. To change that:

      sudo pt35-kbd flash     (undo with: sudo pt35-kbd restore)

  See $SHARE/firmware/README.md.
EOF
  echo
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
        /usr/bin/pt35-session /usr/bin/pt35-kbd /usr/bin/pt35-quick /usr/bin/pt35-hello \
        /etc/keyd/pocketterm35.conf \
        /etc/sudoers.d/pt35-cpu-profile /etc/udev/rules.d/70-pt35-uinput.rules /etc/xdg/fastfetch/config.jsonc \
        /usr/share/wayland-sessions/pt35-session.desktop"
  run "rm -rf '$SHARE' /usr/lib/pt35"
  msg "done. Your ~/.config/{sway,pt35,foot} were left untouched. Reboot."
}

if [ "$UNINSTALL" = 1 ]; then do_uninstall; else do_install; fi
