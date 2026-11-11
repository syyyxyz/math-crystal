#!/usr/bin/env sh
# Build a release and install Math-crystal for the current user:
#   ~/.local/opt/math-crystal/math-crystal   (binary, previous one backed up)
#   ~/.local/bin/math-crystal                (launcher)
#   ~/.local/share/applications/math-crystal.desktop + hicolor icon
set -eu
cd "$(dirname "$0")/.."
taskset -c 16-31 nice cargo build --release -j8

opt="$HOME/.local/opt/math-crystal"
if [ -x "$opt/math-crystal" ]; then
  backup="$HOME/.local/state/math-crystal-backups/math-crystal-$(date +%Y%m%d-%H%M%S)"
  mkdir -p "$backup"
  cp -a "$opt/." "$backup/"
fi
mkdir -p "$opt" "$HOME/.local/bin" "$HOME/.local/share/applications" \
  "$HOME/.local/share/icons/hicolor/256x256/apps"
install -m 755 target/release/math-crystal "$opt/math-crystal.new"
mv -f "$opt/math-crystal.new" "$opt/math-crystal"
install -m 644 assets/icon.png "$HOME/.local/share/icons/hicolor/256x256/apps/math-crystal.png"

cat > "$HOME/.local/bin/math-crystal" <<'SH'
#!/usr/bin/env sh
# Math-crystal launcher. Uses the integrated GPU unless WGPU_POWER_PREF=high.
exec "$HOME/.local/opt/math-crystal/math-crystal" "$@"
SH
chmod 755 "$HOME/.local/bin/math-crystal"

cat > "$HOME/.local/share/applications/math-crystal.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=Math-crystal
Comment=Visual math workbench: multivariable calculus, complex functions, spectra
Exec=$HOME/.local/bin/math-crystal
Icon=math-crystal
Terminal=false
Categories=Education;Science;Math;
StartupWMClass=math-crystal
DESK
command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" || true
echo "Installed: $opt/math-crystal"
