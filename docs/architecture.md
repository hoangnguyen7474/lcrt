# Application Architecture

LCRT keeps portable application behavior in `lcrt-core` and implements host
technology behind small adapter boundaries.

The V1 data path is:

```text
platform audio adapter -> AudioCapture -> CaptionPipeline -> Transcriber
                                              |
                                         CaptionState
                                              |
                                          CaptionSink -> native UI adapter
```

## Portable core

- `audio`: validated PCM chunks, source metadata, and the bounded capture port.
- `transcription`: incremental/final transcript updates and the replaceable STT
  port.
- `caption`: monotonically revisioned caption state and immutable UI snapshots.
- `ui`: a non-blocking native presentation sink boundary.
- `config`: explicit pipeline limits and shutdown responsiveness settings.
- `pipeline`: lifecycle, error propagation, structured tracing, and guaranteed
  audio-stop attempts.

The core contains no PipeWire, GTK, Wayland, WASAPI, or speech-engine APIs.

## Platform adapters

Linux V1 adapters will own PipeWire capture, local whisper.cpp integration, and
GTK4/libadwaita presentation. They translate native failures into actionable
typed boundary errors. Windows adapters can later implement the same ports with
WASAPI and a native Windows UI without changing caption domain logic.

Audio adapters must bound their queues and honor the requested poll timeout.
Transcription work must not run on a UI thread. UI sinks should enqueue or apply
small immutable snapshots and must not perform speech inference.
