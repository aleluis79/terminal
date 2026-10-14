#!/bin/sh
# Install the desktop entry and icon for the current user, so the desktop
# shell (GNOME, KDE, ...) can show the application name and icon for the
# window whose app_id/WM_CLASS is `terminal`.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
data="${XDG_DATA_HOME:-$HOME/.local/share}"
apps="$data/applications"
icons="$data/icons/hicolor/256x256/apps"

mkdir -p "$apps" "$icons"
cp "$here/terminal.desktop" "$apps/terminal.desktop"
cp "$here/terminal.png" "$icons/terminal.png"

command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database "$apps" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -f -t "$data/icons/hicolor" >/dev/null 2>&1 || true

echo "Installed terminal.desktop and terminal.png under $data"
