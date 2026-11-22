#!/usr/bin/env bash
# Launch the standalone layout created by prepare_frosty.py.
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
launcher="$repo/tools/run_frosty.sh"
pointer="$repo/build/frosty-native/latest-independent"
verify_layout=yes
replay_options=()
extra_variables=()
nanite=false
selected_work=
arguments=()
while (( $# )); do
    case $1 in
        --nanite) nanite=true ;;
        --work)
            if (( $# < 2 )); then echo "--work requires a prepared working directory" >&2; exit 2; fi
            selected_work=$(realpath -e -- "$2")
            shift ;;
        *) arguments+=("$1") ;;
    esac
    shift
done
set -- "${arguments[@]}"
if [[ -n $selected_work ]]; then replay_options+=(--work "$selected_work"); fi
if [[ $nanite == true ]]; then replay_options+=(--nanite); fi
# Bridge destruction exceeded 12 GiB while both city and finale assets were live.
desktop_limits=(--property=MemoryHigh=13G --property=MemoryMax=15G --property=MemorySwapMax=2G)
export TUXBOX_SHADER_PREP=${TUXBOX_SHADER_PREP:-menu}
source "$repo/tools/frosty_session.sh"
