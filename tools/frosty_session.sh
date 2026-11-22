# Common Linux session setup, sourced by the retail launchers.
if [[ -n ${selected_work:-} ]]; then
    work=$selected_work
elif [[ -r "$pointer" ]]; then
    work=$(cat -- "$pointer")
else
    echo "Prepare the working copy and prefix for $pointer using docs/MATRIX_AWAKENS.md." >&2
    exit 1
fi
layout=$(cat -- "$work/stage")
prefix="$work/prefix"
if [[ -s "$work/proton" && ( $verify_layout == yes || -z ${PROTONPATH:-} ) ]]; then
    PROTONPATH=$(cat -- "$work/proton")
    export PROTONPATH
fi
if [[ ! "$prefix/dosdevices/g:" -ef "$layout" ]]; then
    echo "The prefix G: drive must point at the staged package in $work/stage." >&2
    exit 1
fi

if [[ ${1:-} == --desktop ]]; then
    shift
    description="Frosty in $work"
    if [[ $verify_layout == yes && $nanite == true ]]; then description+=" (Nanite)"; fi
    # A separate service keeps the game alive across editor restarts. The
    # unit name also lets a second desktop invocation find the existing game.
    if systemctl --user is-active --quiet tuxbox-frosty.service; then
        if [[ $(systemctl --user show tuxbox-frosty.service --property=Description --value) != "$description" ]]; then
            echo "Another Frosty configuration is running in tuxbox-frosty.service." >&2
            exit 1
        fi
        echo "Frosty is already running in tuxbox-frosty.service."
        echo "Follow the live console with journalctl --user -fu tuxbox-frosty.service -o cat"
        exit 0
    fi
    export TUXBOX_SHADER_PREP_UI=${TUXBOX_SHADER_PREP_UI:-1}
    environment=()
    for var in DISPLAY XAUTHORITY PULSE_SERVER PROTONPATH PROTON_LOG_DIR WINEDEBUG \
               TUXBOX_LOG TUXBOX_MANGOHUD TUXBOX_MANGOHUD_PCI TUXBOX_SHADER_PREP TUXBOX_SHADER_PREP_UI TUXBOX_XSHADER DXC \
               TUXBOX_RUNTIME_SHADERS TUXBOX_TRACE_SHADERS TUXBOX_TRACE_STENCIL "${extra_variables[@]}" TUXBOX_KEYBOARD_GAMEPAD VK_LAYER_PATH VK_ADD_LAYER_PATH \
               VK_INSTANCE_LAYERS VKD3D_CONFIG VKD3D_DISABLE_EXTENSIONS PRESSURE_VESSEL_FILESYSTEMS_RO; do
        if [[ -v $var ]]; then environment+=("--setenv=$var=${!var}"); fi
    done
    systemd-run --user --collect --unit=tuxbox-frosty --description="$description" --working-directory="$repo" \
        "${desktop_limits[@]}" \
        --property=StandardOutput=journal --property=StandardError=journal \
        "${environment[@]}" "$launcher" "${replay_options[@]}" "$@"
    echo "Follow the live console with journalctl --user -fu tuxbox-frosty.service -o cat"
    exit 0
fi

exec 9>"$work/launch.lock"
if ! flock --nonblock 9; then
    echo "Frosty is already running with this prefix." >&2
    exit 1
fi

if [[ $verify_layout == yes ]]; then
    python "$repo/tools/prepare_frosty.py" --check --work "$work"
fi

# The Construct map plays its intro through Electra with audio enabled.
run_options=()
case ${1:-} in
    --prepare-only|--no-shader-prep) run_options+=("$1"); shift ;;
esac
if (( $# == 0 )) && [[ ${run_options[0]:-} != --prepare-only ]]; then
    set -- -DisablePlugins=XboxCommonGDKMoviePlayer
    # A device can answer PulseAudio queries and still stall on actual playback.
    audio_ready=true
    if command -v pacat >/dev/null; then
        head -c 19200 /dev/zero | timeout -k 1 3 pacat --playback --raw --rate=48000 \
            --channels=2 --format=s16le --latency-msec=80 --client-name=Frosty-output-check \
            >/dev/null 2>&1 || audio_ready=false
    elif command -v pactl >/dev/null; then
        timeout -k 1 3 pactl get-default-sink >/dev/null 2>&1 || audio_ready=false
    fi
    if [[ $audio_ready == false ]]; then
        echo "The desktop audio output did not respond. Starting Frosty muted." >&2
        set -- "$@" -nosound
    fi
fi
if [[ ${run_options[0]:-} != --prepare-only && " ${*,,} " != *" -nullrhi "* ]]; then
    if [[ ${profile_pipelines:-false} == true ]]; then run_options+=(--profile-pipelines); fi
    case ${TUXBOX_MANGOHUD:-auto} in
        auto) if command -v mangohud >/dev/null; then run_options+=(--mangohud); fi ;;
        1) run_options+=(--mangohud) ;;
        0) ;;
        *) echo "TUXBOX_MANGOHUD must be auto, 1 or 0." >&2; exit 2 ;;
    esac
fi
logdir=${PROTON_LOG_DIR:-$(mktemp -d "$work/launch.XXXXXX")}
mkdir -p -- "$logdir"
logdir=$(cd -- "$logdir" && pwd)
printf 'Frosty launch log directory: %s\n' "$logdir"
status=0
default_debug='-all,+timestamp,err+all,warn+debugstr,warn+seh'
namespace=(unshare --user --map-current-user --net)
if [[ $verify_layout == yes && $nanite == true ]]; then
    config="$layout/Frosty/Config/UserEngine.ini"
    override=$(mktemp -- "$logdir/UserEngine.XXXXXXXX.ini")
    python - "$config" "$override" <<'PY'
from pathlib import Path
import re
import sys

source, target = map(Path, sys.argv[1:])
contents = source.read_bytes()
lines = contents.splitlines(keepends=True)
active = updated = False
for i, line in enumerate(lines):
    if line.lstrip().startswith(b"["):
        active = line.strip().lower() == b"[consolevariables]"
    if active:
        lines[i], count = re.subn(rb"(?i)^([ \t]*r\.Nanite[ \t]*=[ \t]*)[^;#\s]+", rb"\g<1>1", line)
        updated |= bool(count)
if not updated:
    newline = b"\r\n" if b"\r\n" in contents else b"\n"
    lines.extend([newline, b"[ConsoleVariables]", newline, b"r.Nanite=1", newline])
target.write_bytes(b"".join(lines))
PY
    namespace+=(--mount --propagation private --keep-caps bash -ec '
        mount --bind "$1" "$2"
        shift 2
        exec setpriv --inh-caps=-all --ambient-caps=-all "$@"
    ' frosty-nanite "$override" "$config")
fi
"${namespace[@]}" env -u WAYLAND_DISPLAY \
    TUXBOX_KEYBOARD_GAMEPAD="${TUXBOX_KEYBOARD_GAMEPAD:-1}" \
    PROTON_LOG=1 PROTON_LOG_DIR="$logdir" WINEDEBUG="${WINEDEBUG-$default_debug}" \
    UMU_RUNTIME_UPDATE=0 WINEDLLOVERRIDES="mfplat=n,b;gameinput=n,b${WINEDLLOVERRIDES:+;$WINEDLLOVERRIDES}" \
    python "$repo/tuxbox.py" run --backend proton \
    --prefix "$prefix" --log-file "$logdir/tuxbox.log" "${run_options[@]}" "$layout" "$@" || status=$?
# Some Unreal fatal exits appear to umu as success. The native title records
# the fatal diagnostic and the shutdown request on the same thread.
if [[ ${run_options[0]:-} != --prepare-only && $status == 0 && -f "$logdir/steam-default.log" ]] && \
   awk -F: '/:warn:seh:OutputDebugStringW L"Fatal error:|:warn:seh:OutputDebugStringW .*LogCore: Error: Fatal error!/ {fatal[$2]=1}
            /:warn:seh:OutputDebugStringW .*FPlatformMisc::RequestExit\(1\)/ && fatal[$2] {failed=1}
            END {exit !failed}' "$logdir/steam-default.log"; then
    echo "Frosty reported a fatal error. See $logdir/steam-default.log" | tee -a "$logdir/tuxbox.log" >&2
    status=1
fi
exit "$status"
