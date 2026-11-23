# Progress and compatibility

The overall bar counts verified milestones on the seven-step project roadmap below. Package identification is verified. Four milestones have partial results, and retail gameplay and multi-title performance are still open. Each milestone earns one point when verified.

The Frosty bar measures eight milestones on Linux with the packaged graphics settings. The latest TuxBox run loaded the Construct world and submitted a 3840 × 2160 frame. The frame was black. Lumen compute shader 219 stopped the run before the menu. [Progress map](progress.svg) · [Retail comparison](../evidence/matrix-awakens/three-library-comparison.json)

The six remaining milestones are an operable menu, a playable chase, open-city exploration, correct visuals, working game audio and verified frame pacing. Visual checks cover the menu code, hair, vehicles and buildings. Frame pacing needs a sustained gameplay capture with MangoHud. Earlier reduced-settings runs are recorded separately. Cross-title compatibility remains a later project milestone. [Game results](MATRIX_AWAKENS.md) · [Shader translation](SHADERS.md)

## Games

| Title | Current result | What remains |
| --- | --- | --- |
| **Lanes** | Runs through TuxBox's Xbox graphics, memory and input providers. Its replay checks GPU output and game state. | Continue testing presentation, input and other providers across systems. [Lanes](../examples/lanes/README.md) |
| **The Matrix Awakens (Frosty)** | With the original Xbox graphics settings, the latest recorded Linux run loaded the Construct map and presented a black frame before stopping during Lumen shader creation. Earlier runs using reduced graphics settings reached the chase and city, but showed missing and distorted surfaces. | Support the inline ray-traversal shader and validate the full game image, controls and performance with the original settings. [Setup and results](MATRIX_AWAKENS.md) · [Recorded original-settings run](../evidence/matrix-awakens/packaged-renderer.json) |
| **Supplied ProjectScarlett package** | The declared game executable was not present when inspected. Its packaged DLLs were inventoried, but the game was not launched. | Inspect and test the executable when it is available. |
| **Fortnite** | One supplied build provided graphics API call-site evidence. | Test initialization and rendering in TuxBox for a specific build. [Device evidence](../evidence/fortnite-device.json) |

No other retail title is currently claimed playable. Record the build, settings, host, provider and observed faults for each run, and distinguish starting, rendering, input and sustained gameplay.

## Longer-term project milestones

| Milestone | Status | Evidence | Next step |
| --- | --- | --- | --- |
| 0. Identify a package | Verified | Frosty packages and DLL exports inventoried | Track dynamic loads for each title |
| 1. Native process and memory | Partial | Memory allocation and working-set controls pass on Proton | Compare on physical Windows |
| 2. Guest graphics submission | Partial | Xbox device, command list and presentation controls pass on selected hosts | Extend method and adapter coverage |
| 3. Retail shader and resource translation | Partial | Packaged shader pipelines and individual GPU outputs tested | Verify full-frame rendering and missing shaders |
| 4. Playable platform services | Partial | Lanes tests input and selected audio paths | Test hotplug, persistence and title behavior |
| 5. First retail title | Original settings blocked | Frosty stops at a Lumen shader after a black frame | Reach a correct interactive game image |
| 6. Multi-title compatibility and performance | Pending | No second retail title has a recorded launch | Test more titles and compare frame times |

## What is tested

- **Xbox libraries:** export counts show the identified functions, while individual provider checks cover only the behavior exercised. [Library coverage](LIBRARIES.md)
- **Graphics and shaders:** translated HLSL, validated DXIL, created pipelines, GPU readbacks and a correct game image are separate milestones. The recorded surveys identify the archive and compiler they used. [Shader coverage](SHADERS.md) · [Graphics provider](../runtime/d3d12/README.md)
- **Input and audio:** Lanes exercises keyboard, mouse and controller input. Separate audio controls test selected channel routing; this does not establish every game's input or audio behavior. [Lanes](../examples/lanes/README.md) · [Results by system](RESULTS.md)
- **Platforms:** Linux results identify the Proton build and GPU. Physical Windows remains a separate target and must be tested separately. [Results by system](RESULTS.md)

## Current priorities

1. Make Frosty's original graphics profile progress past the inline Lumen ray-traversal shader, including correct acceleration-structure addresses and GPU lifetimes.
2. Check the menu, characters, hair, vehicles and city for rendering errors in the actual game. Record known defects and test input and audio.
3. Measure cold and warm frame times and address hitches without changing the game's graphics settings.
4. Repeat title and provider checks on additional hardware and games as their executables become available.

The default compiler still contains a TomBox-derived HLSL emitter. TuxBox's independent emitter is growing, but it does not yet replace the legacy path for all supported shaders. [Source attribution](REFERENCES.md#shader-compiler-attribution)
