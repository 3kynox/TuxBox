#!/usr/bin/env bash
# Launch the standalone layout created by prepare_frosty.py.
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
launcher="$repo/tools/run_frosty.sh"
pointer="$repo/build/frosty-native/latest-independent"
verify_layout=yes
replay_options=()
extra_variables=()
export TUXBOX_SHADER_PREP=${TUXBOX_SHADER_PREP:-menu}
source "$repo/tools/frosty_session.sh"
