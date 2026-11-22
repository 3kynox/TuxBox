# Common Linux session setup, sourced by the retail launchers.
if [[ ! -r "$pointer" ]]; then
    echo "Prepare the working copy and prefix for $pointer using docs/MATRIX_AWAKENS.md." >&2
    exit 1
fi
work=$(cat -- "$pointer")
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
    # A separate service keeps the game alive across editor restarts. The
    # unit name also lets a second desktop invocation find the existing game.
    if systemctl --user is-active --quiet tuxbox-frosty.service; then
        if [[ $(systemctl --user show tuxbox-frosty.service --property=Description --value) != "Frosty in $work" ]]; then
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
               TUXBOX_LOG TUXBOX_SHADER_PREP TUXBOX_SHADER_PREP_UI TUXBOX_XSHADER DXC \
               TUXBOX_RUNTIME_SHADERS TUXBOX_TRACE_SHADERS TUXBOX_TRACE_STENCIL "${extra_variables[@]}" TUXBOX_KEYBOARD_GAMEPAD VK_LAYER_PATH VK_ADD_LAYER_PATH \
               VK_INSTANCE_LAYERS VKD3D_DISABLE_EXTENSIONS PRESSURE_VESSEL_FILESYSTEMS_RO; do
        if [[ -v $var ]]; then environment+=("--setenv=$var=${!var}"); fi
    done
    systemd-run --user --collect --unit=tuxbox-frosty --description="Frosty in $work" --working-directory="$repo" \
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

# Frosty's console constant-buffer views can start at any 16-byte boundary.
# With descriptor sets, vkd3d can use storage buffers on adapters that need it.
layer="$repo/build/frosty-cbv-layer"
if [[ ! -r "$layer/frosty.json" || ! -r "$layer/frosty_cbv.so" ]]; then
    "$repo/tools/build_frosty_cbv_layer.sh" "$layer"
fi
if [[ -n ${VK_LAYER_PATH:-} ]]; then
    export VK_LAYER_PATH="$layer:$VK_LAYER_PATH"
else
    export VK_ADD_LAYER_PATH="$layer${VK_ADD_LAYER_PATH:+:$VK_ADD_LAYER_PATH}"
fi
export VK_INSTANCE_LAYERS="VK_LAYER_TUXBOX_console_cbv${VK_INSTANCE_LAYERS:+:$VK_INSTANCE_LAYERS}"
export VKD3D_DISABLE_EXTENSIONS="VK_EXT_descriptor_buffer${VKD3D_DISABLE_EXTENSIONS:+,$VKD3D_DISABLE_EXTENSIONS}"
export PRESSURE_VESSEL_FILESYSTEMS_RO="$layer${PRESSURE_VESSEL_FILESYSTEMS_RO:+:$PRESSURE_VESSEL_FILESYSTEMS_RO}"

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
logdir=${PROTON_LOG_DIR:-$(mktemp -d "$work/launch.XXXXXX")}
mkdir -p -- "$logdir"
logdir=$(cd -- "$logdir" && pwd)
printf 'Frosty launch log directory: %s\n' "$logdir"
status=0
default_debug='-all,+timestamp,err+all,warn+debugstr,warn+seh'
unshare --user --map-current-user --net env -u WAYLAND_DISPLAY \
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
