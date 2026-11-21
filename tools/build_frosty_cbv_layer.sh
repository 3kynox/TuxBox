#!/usr/bin/env bash
# Compile the explicit, app-local layer against Vulkan-Headers on the Linux host.
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
src="$repo/tools/reference/cbv-layer"
out="${1:-$repo/build/frosty-cbv-layer}"
include=()
if [[ -n "${VULKAN_HEADERS_INCLUDE:-}" ]]; then
    include=(-I "$VULKAN_HEADERS_INCLUDE")
elif [[ ! -f /usr/include/vulkan/vk_layer.h &&
        -f "$repo/build/frosty-signature/chase-vertex/vkd3d-proton/khronos/Vulkan-Headers/include/vulkan/vk_layer.h" ]]; then
    include=(-I "$repo/build/frosty-signature/chase-vertex/vkd3d-proton/khronos/Vulkan-Headers/include")
fi
mkdir -p -- "$out"
stage=$(mktemp -d -- "$out/.build.XXXXXXXX")
trap 'rm -rf -- "$stage"' EXIT
"${CC:-cc}" -std=c11 -Wall -Wextra -Werror -O2 -fPIC -shared -pthread \
    "${include[@]}" "$src/layer.c" -o "$stage/frosty_cbv.so"
cp -- "$src/frosty.json" "$stage/frosty.json"
mv -f -- "$stage/frosty_cbv.so" "$out/frosty_cbv.so"
mv -f -- "$stage/frosty.json" "$out/frosty.json"
