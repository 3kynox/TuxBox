#!/usr/bin/env bash
# Launch the standalone layout created by prepare_frosty.py.
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
launcher="$repo/tools/run_frosty.sh"
pointer="$repo/build/frosty-native/latest-independent"
verify_layout=yes
replay_options=()
extra_variables=()
# Bridge destruction exceeded 12 GiB while both city and finale assets were live.
desktop_limits=(--property=MemoryHigh=13G --property=MemoryMax=15G --property=MemorySwapMax=2G)
export TUXBOX_SHADER_PREP=${TUXBOX_SHADER_PREP:-menu}
source "$repo/tools/frosty_session.sh"
