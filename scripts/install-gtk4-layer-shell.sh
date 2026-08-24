#!/usr/bin/env bash
set -euo pipefail

readonly VERSION="1.0.4"
readonly ARCHIVE_SHA256="7fe327dc3740e4b6f5edfd855e23f84b1ac1ec6854b731047b95df7feb46498b"
readonly ARCHIVE_URL="https://github.com/wmww/gtk4-layer-shell/archive/refs/tags/v${VERSION}.tar.gz"

if pkg-config --atleast-version="${VERSION}" gtk4-layer-shell-0; then
  printf 'gtk4-layer-shell %s or newer is already installed\n' "${VERSION}"
  exit 0
fi

work_dir="$(mktemp -d)"
trap 'rm -rf -- "${work_dir}"' EXIT
archive="${work_dir}/gtk4-layer-shell.tar.gz"

curl --location --fail --silent --show-error "${ARCHIVE_URL}" --output "${archive}"
printf '%s  %s\n' "${ARCHIVE_SHA256}" "${archive}" | sha256sum --check --status
tar --extract --gzip --file "${archive}" --directory "${work_dir}"

meson setup "${work_dir}/build" "${work_dir}/gtk4-layer-shell-${VERSION}" \
  --buildtype=release \
  --prefix=/usr/local \
  -Ddocs=false \
  -Dexamples=false \
  -Dintrospection=false \
  -Dsmoke-tests=false \
  -Dtests=false \
  -Dvapi=false
CCACHE_DISABLE=1 meson compile -C "${work_dir}/build"
sudo meson install -C "${work_dir}/build"
sudo ldconfig
