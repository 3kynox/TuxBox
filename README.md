# TuxBox

TuxBox is an experimental project for running Xbox Series X|S games on PC. On Linux it runs with Valve’s stock Proton Experimental. Windows support is also in development.

## What runs today?

- **Lanes** is a small game that uses TuxBox’s Xbox graphics, memory and input libraries. Collect coins and dodge barriers using a keyboard, mouse or Xbox controller.
- **The Matrix Awakens (Frosty)** reaches the Wake Up chase cinematic on Linux with TuxBox XG and reduced graphics settings. Earlier reduced-settings runs reached the highway chase and open city. With the original Xbox settings it loads the Construct map and presents a black frame before stopping while creating a compute shader during scene setup. You supply the game, its matching Xbox system files and Proton. [Setup and test results](docs/MATRIX_AWAKENS.md)

## Try Lanes

Once the build tools are installed, run these commands from the project folder. [Build requirements](examples/lanes/README.md#build-requirements)

```sh
python -m pip install -r requirements.txt
python build.py lanes
python tuxbox.py run --app-id Lanes build/lanes
```

Press Left or Right to change lanes, or Space to take a turn in your current lane. R restarts the game and Escape quits.

## Game logs

`tuxbox.py run` shows game output in the terminal as it arrives. The first line gives the path to a saved copy. On Linux, logs go in `~/.local/state/project-tuxbox/logs/` (or under `XDG_STATE_HOME`). On Windows, they go in `%LOCALAPPDATA%\TuxBox\logs\`.

To choose a file, pass `--log-file` before the game path. Each run appends to that file.

```sh
python tuxbox.py run --log-file build/lanes.log --app-id Lanes build/lanes
```

On Linux, add `--mangohud` before the game path to see FPS, frame times and hardware load while playing. TuxBox saves a CSV of presented frames and a `summary.json` with average FPS and median, p95 and p99 frame times in a capture folder beside the game log. The Frosty launcher enables this automatically when MangoHud is installed.

Run `python tuxbox.py inspect /path/to/game` to see the executable TuxBox will launch and what it found in the package’s `.bin` files. For Frosty, its shipping symbols can also turn addresses from the log into function names. [Symbol lookup example](docs/MATRIX_AWAKENS.md#what-carries-over-to-projectscarlett)

## Compatibility

[![Verified game behavior and Xbox Series shader translation measured on Frosty](docs/progress.svg)](docs/progress.svg)

[Current progress](docs/PROGRESS.md) · [Xbox libraries](docs/LIBRARIES.md) · [Shader coverage](docs/SHADERS.md) · [Test results](docs/RESULTS.md) · [Architecture](docs/ARCHITECTURE.md) · [Credits and references](docs/REFERENCES.md)
