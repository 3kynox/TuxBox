# PIX no-capturer support

The Rust `pixevt.dll` reports that no PIX capture is active. The reviewed Series DLL returns `E_NOTIMPL` from `PIXBeginCapture1` and `E_UNEXPECTED` from `PIXBeginCapture2` and `PIXEndCapture` when no capture backend is attached. TuxBox returns those same results; it does not create PIX captures or implement backend registration.

`PIXReportCounter` can emit title counters to the live log when `TUXBOX_TRACE_PIX=1`. With `TUXBOX_LOG=1` it uses the normal Wine debug channel; otherwise it writes to standard error. Counter reporting is disabled by default. The provider currently implements five of the reviewed DLL's 16 named exports and is not staged by default for games that may need the remaining functions.

Build with `python build.py pixevt`. Compare a locally supplied Xbox DLL with the TuxBox provider:

```sh
python tools/check_pixevt.py --reference /path/to/pixevt.dll --xmem build/xmem-smoke/xmem.dll \
    --provider build/pixevt/pixevt.dll --proton "/path/to/Proton - Experimental" \
    --prefix build/pixevt-prefix --output build/pixevt-control.json
```

The control checks names and ordinals, capture states, four capture flag values and guarded parameters. It also verifies that counter messages appear only with tracing enabled. [Recorded control](../../evidence/providers/pixevt-no-capturer.json) · [Microsoft PIX capture API](https://learn.microsoft.com/en-us/windows/pix/get-started/programmatic-capture)
