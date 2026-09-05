# V1 Whisper baseline

This document defines and records the reproducible V1 Whisper baseline at the
tested commit. It deliberately separates paced real-time latency from
fastest-possible offline throughput.

## Benchmark contract

- `model_startup_ms`: wall-clock time from immediately before
  `WhisperTranscriber::new` until the local backend reports that it is ready to
  accept audio. WAV decoding is excluded.
- `first_partial_ms`: in `paced` mode, wall-clock time from the start of paced
  replay until the diagnostic receives the first non-empty `Partial` update.
- `first_final_ms`: in `paced` mode, wall-clock time from the start of paced
  replay until the diagnostic receives the first non-empty `Final` update.
- `completion_ms`: wall-clock time from replay start until `finish()` returns
  after fully flushing the input.
- `real_time_factor`: in `offline` mode only, audio push and finalization wall
  time divided by WAV duration. Model startup and WAV decoding are excluded.
- `inference_count`: successful Whisper inference passes reported by the
  existing backend counter.
- Peak memory and CPU: GNU `/usr/bin/time` maximum resident set size and
  `percent_of_cpu`. The CPU percentage is a process-wide average over elapsed
  wall time, not per-core sampling.
- Transcript: the text of the last transcript update. No accuracy score is
  reported unless a trustworthy reference transcript is available.

Paced mode delivers each bounded WAV chunk only when its corresponding audio
duration has elapsed. It therefore measures latency under simulated real-time
delivery; it is not an audio-playback or speech-onset measurement. Offline mode
delivers chunks as quickly as bounded backpressure allows and is the only mode
used for throughput RTF.

## Results

To be completed from controlled measurements on the milestone commit.
