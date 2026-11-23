# Progress and compatibility

The overall bar gives equal weight to verified library behavior and shader translation. The library percentage counts the reviewed behaviors Frosty can perform. The shader percentage counts entries in its surveyed archives that produced compiler output. Each library behavior counts once, including paths that use packaged or host DLLs. Each verified behavior needs an observable result from the game or a focused check. The percentage describes this workload and moves as games expose new requirements. [Progress map](progress.svg) · [Library audit](LIBRARIES.md) · [Shader survey](SHADERS.md)

With reduced graphics settings, TuxBox reached the Wake Up cinematic and loaded the bullet-time scene. With the packaged settings, the game presented a black frame and stopped while building Lumen shader 219. Gameplay, image quality, audio and frame pacing are checked in retail runs. [Wake Up run](../evidence/matrix-awakens/wake-up-xg.json) · [Original-settings comparison](../evidence/matrix-awakens/three-library-comparison.json)

## Games

| Title | Current result | What remains |
| --- | --- | --- |
| **Lanes** | Runs through TuxBox's Xbox graphics, memory and input providers. Its replay checks GPU output and game state. | Continue testing presentation, input and other providers across systems. [Lanes](../examples/lanes/README.md) |
| **The Matrix Awakens (Frosty)** | With the original Xbox graphics settings, the latest recorded Linux run loaded the Construct map and presented a black frame before stopping during Lumen shader creation. The current reduced-settings build reached Wake Up. Earlier reduced-settings runs reached the highway and city with missing and distorted surfaces. | Support the inline ray-traversal shader and validate the full game image, controls and performance with the original settings. [Setup and results](MATRIX_AWAKENS.md) · [Recorded original-settings run](../evidence/matrix-awakens/packaged-renderer.json) |
| **Supplied ProjectScarlett package** | The declared game executable was not present when inspected. Its packaged DLLs were inventoried, but the game was not launched. | Inspect and test the executable when it is available. |
| **Fortnite** | One supplied build provided graphics API call-site evidence. | Test initialization and rendering in TuxBox for a specific build. [Device evidence](../evidence/fortnite-device.json) |

No other retail title is currently claimed playable. Record the build, settings, host, provider and observed faults for each run, and distinguish starting, rendering, input and sustained gameplay.

## Current priorities

1. Make Frosty's original graphics profile progress past the inline Lumen ray-traversal shader, by supplying the Xbox-format scene nodes that shader 219 reads. Check its shadow mask on an empty tile, a hit and a miss before rerunning Frosty. [Shader 219 comparison](SHADERS.md#inline-lumen-ray-tracing)
2. Check the menu, characters, hair, vehicles and city for rendering errors in the actual game. Record known defects and test input and audio.
3. Measure cold and warm frame times and address hitches without changing the game's graphics settings.
4. Repeat title and provider checks on additional hardware and games as their executables become available.

The default compiler still contains a TomBox-derived HLSL emitter. TuxBox's independent emitter is growing, but it does not yet replace the legacy path for all supported shaders. [Source attribution](REFERENCES.md#shader-compiler-attribution)
