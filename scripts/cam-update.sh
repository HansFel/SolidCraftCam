#!/usr/bin/env bash
# CAM fork only: installs or updates SolidCraft CAM on a Linux PC from the `cam-latest`
# release that .github/workflows/cam-build.yml publishes, without compiling anything.
#
#   scripts/cam-update.sh            the desktop app into ~/.local/opt/solidcraft-cam
#   scripts/cam-update.sh --web      also rebuild and restart the web container
#
# The desktop app needs no root: it goes under ~/.local with a menu entry "SolidCraft CAM".
# With --web the static site is put into the nginx image of packaging/docker (seconds, no
# Rust), tagged solidcraft-web:<commit>, and the container $WEB_NAME is replaced on
# $WEB_PORT; the previous image stays for going back. Run it from a checkout of the fork.
set -euo pipefail

REPO="${CAM_REPO:-HansFel/SolidCraftCam}"
TAG="${CAM_TAG:-cam-latest}"
DEST="${CAM_DEST:-$HOME/.local/opt/solidcraft-cam}"
WEB_NAME="${WEB_NAME:-solidcraft-public}"
WEB_PORT="${WEB_PORT:-8021}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
URL="https://github.com/$REPO/releases/download/$TAG"

WEB=0
for arg in "$@"; do
  case "$arg" in
    --web) WEB=1 ;;
    -h | --help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

COMMIT="$(curl -fsSL "$URL/COMMIT")"
echo "==> SolidCraft CAM $COMMIT from $REPO ($TAG)"

if [ -f "$DEST/COMMIT" ] && [ "$(cat "$DEST/COMMIT")" = "$COMMIT" ]; then
  echo "==> desktop app already at $COMMIT"
else
  echo "==> downloading the desktop app"
  curl -fL --progress-bar -o "$TMP/app.tar.gz" "$URL/solidcraft-cam-linux-x86_64.tar.gz"
  mkdir -p "$TMP/app"
  tar -xzf "$TMP/app.tar.gz" -C "$TMP/app" --strip-components=1
  test -x "$TMP/app/bin/solidcraft"
  rm -rf "$DEST.old"
  if [ -d "$DEST" ]; then mv "$DEST" "$DEST.old"; fi
  mkdir -p "$(dirname "$DEST")"
  mv "$TMP/app" "$DEST"
  printf '%s\n' "$COMMIT" > "$DEST/COMMIT"
  echo "==> installed in $DEST (previous version: $DEST.old)"
fi

ICON="$(find "$DEST/share/icons" -name '*.png' -path '*256x256*' 2>/dev/null | head -n 1)"
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/solidcraft-cam.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=SolidCraft CAM
Comment=SolidCraft with CAM ($COMMIT)
Exec=$DEST/bin/solidcraft %F
Icon=${ICON:-applications-engineering}
Terminal=false
Categories=Graphics;Engineering;
EOF
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "==> menu entry: SolidCraft CAM"

if [ "$WEB" = 1 ]; then
  echo "==> downloading the web app"
  curl -fL --progress-bar -o "$TMP/web.tar.gz" "$URL/solidcraft-cam-web.tar.gz"
  mkdir -p "$TMP/site"
  tar -xzf "$TMP/web.tar.gz" -C "$TMP/site"
  test -f "$TMP/site/index.html"
  IMAGE="solidcraft-web:$COMMIT"
  docker build -q -f "$ROOT/packaging/docker/Dockerfile" --build-context site="$TMP/site" -t "$IMAGE" "$ROOT" >/dev/null
  OLD="$(docker inspect --format '{{.Config.Image}}' "$WEB_NAME" 2>/dev/null || true)"
  docker rm -f "$WEB_NAME" >/dev/null 2>&1 || true
  docker run -d --name "$WEB_NAME" --restart unless-stopped -p "$WEB_PORT:8080" "$IMAGE" >/dev/null
  for _ in $(seq 1 30); do
    if curl -fs -o /dev/null "http://localhost:$WEB_PORT/"; then break; fi
    sleep 1
  done
  curl -fs -o /dev/null "http://localhost:$WEB_PORT/"
  echo "==> web app $IMAGE on port $WEB_PORT${OLD:+ (back: docker rm -f $WEB_NAME && docker run -d --name $WEB_NAME --restart unless-stopped -p $WEB_PORT:8080 $OLD)}"
fi
