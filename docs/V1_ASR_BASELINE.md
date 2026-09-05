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

### Test identity

- Benchmark implementation commit: `a683172e0778f3c7582e6ada0475fc87676f149e`.
  Only this report was changed after the measurements; the measured binary
  source was unchanged.
- Build: Cargo `release`, repository-pinned Rust 1.98.0
  (`88d9e12ae178fab0fb5cc050a94da85685d449ea`), CPU-only Whisper defaults with
  four inference threads.
- OS: Ubuntu 26.04 LTS, Linux 7.0.0-30-generic, x86_64.
- CPU: 12th Gen Intel Core i5-12500H, 16 logical CPUs.
- Installed RAM: 40,659,587,072 bytes (37.87 GiB).
- Model: `ggml-tiny.en.bin`, 77,704,715 bytes, SHA-256
  `921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f`.
- Language: `en`.
- Sample: upstream whisper.cpp `samples/jfk.wav`, restored from the same
  project test source used by earlier LCRT milestones; 352,078 bytes, SHA-256
  `59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e`,
  11.000 seconds, signed 16-bit PCM, 16 kHz, mono. `silencedetect` found no
  silence of at least 100 ms at -40 dB, so there is no detected meaningful
  leading silence under that stated threshold.

The machine was allowed to settle for 10 seconds before each series. One
warm-up per mode was excluded, followed by five sequential measured runs. No
Cargo build or other deliberately started workload overlapped the runs. Five
samples do not justify a p95 claim, so the report uses min/median/max.

### Paced real-time replay

The diagnostic delivered 20 ms chunks according to cumulative sample duration.
All times below are wall-clock milliseconds from the benchmark JSON. GNU
`time` CPU is rounded whole-process average CPU utilization; RSS is peak KiB
for the process.

| Run | Startup | First partial | First final | Completion | Inferences | CPU | Peak RSS KiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 61.968 | 2320.279 | 11616.322 | 11616.323 | 7 | 135% | 221,176 |
| 2 | 61.418 | 2340.337 | 11613.867 | 11613.868 | 7 | 136% | 221,420 |
| 3 | 60.637 | 2340.069 | 11611.387 | 11611.388 | 7 | 139% | 220,884 |
| 4 | 59.570 | 2340.294 | 11606.303 | 11606.304 | 7 | 137% | 221,136 |
| 5 | 60.064 | 2340.071 | 11606.285 | 11606.286 | 7 | 137% | 221,096 |
| **Median** | **60.637** | **2340.071** | **11611.387** | **11611.388** | **7** | **137%** | **221,136** |
| **Min-max** | **59.570-61.968** | **2320.279-2340.337** | **11606.285-11616.322** | **11606.286-11616.323** | **7-7** | **135-139%** | **220,884-221,420** |

Across all ten measured paced and offline process starts, startup was 59.959 ms
median (56.964-87.523 ms). The overall measured peak was 221,564 KiB
(216.37 MiB). The paced final arrives during explicit end-of-input flush because
this fixture does not contain the configured 900 ms final-silence interval.

Every paced run produced:

> And so, my fellow Americans Ask not what your country can do for you. Ask what
> you can do for your country.

After lowercasing, removing punctuation, and collapsing whitespace, this is an
exact match to the reference quote in all 5/5 paced runs. This exact-match check
is specific to the known JFK quote; no broader accuracy claim follows from one
sample.

### Fastest-possible offline throughput

Offline input used the pre-existing 4,096-frame chunks and bounded producer
backpressure. Completion and RTF exclude model startup and WAV decoding. The
partial/final timestamps emitted in raw offline JSON are not latency results
and are deliberately omitted here.

| Run | Startup ms | Completion ms | RTF | Inferences | CPU | Peak RSS KiB | Normalized exact match |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 59.854 | 3506.015 | 0.318729 | 6 | 386% | 221,416 | yes |
| 2 | 58.816 | 2963.786 | 0.269435 | 5 | 385% | 221,564 | yes |
| 3 | 56.964 | 3511.369 | 0.319215 | 6 | 386% | 221,336 | yes |
| 4 | 58.827 | 3605.859 | 0.327805 | 6 | 386% | 221,268 | yes |
| 5 | 87.523 | 4161.861 | 0.378351 | 5 | 384% | 221,548 | no |
| **Median** | **58.827** | **3511.369** | **0.319215** | **6** | **386%** | **221,416** | **4/5** |
| **Min-max** | **56.964-87.523** | **2963.786-4161.861** | **0.269435-0.378351** | **5-6** | **384-386%** | **221,268-221,564** | **n/a** |

Run 5 emitted a duplicated leading fragment before the otherwise correct full
sentence. That real variability is retained as a baseline limitation rather
than discarded. Offline RTF is the throughput result; paced completion divided
by audio duration is intentionally not presented as throughput.

## Integrated Ubuntu acceptance

- Source discovery found the real built-in analog stereo microphone and the
  matching system-output monitor through both `lcrt-pw-capture` and `lcrt`.
- A five-second microphone -> Whisper -> GTK smoke delivered 227 audio chunks,
  zero captions, and exited cleanly. The microphone remained silent/muted; this
  validates plumbing and shutdown, not spoken-language accuracy.
- A 15-second system-output -> Whisper -> GTK smoke replayed the verified JFK
  fixture, delivered 437 chunks and seven caption updates, and exited cleanly.
- Three further two-second microphone Start/Stop smoke cycles delivered 86, 90,
  and 90 chunks. Every subsequent launch remained usable, every exit succeeded,
  and inspection found no remaining LCRT process or PipeWire node. These were
  separate bounded app launches, not repeated cycles in one retained window.
- The deterministic cancellation regression
  `cancellation_winning_at_the_audio_boundary_does_not_start_audio` passed. This
  is unit-test evidence that a Stop winning startup prevents audio acquisition,
  not a hardware-timed reproduction.
- The release GTK diagnostic performed a programmatic normal close in 1.43
  seconds with exit status 0. No human mouse click was claimed.

The GNOME Wayland compositor did not advertise layer shell, so these runs used
the compositor-managed standard-window fallback. Visual appearance was not
assessed.

## Offline/no-network evidence

The full 11-second offline transcription completed successfully inside a
`bubblewrap --unshare-net` network namespace. This isolated only that process;
the workstation network was not changed and root access was not used. The run
loaded the configured local model and completed with five inference passes and
the expected final transcript.

Code and dependency inspection found no network client in the application,
audio, core, UI, or Whisper runtime crates and no implicit download path.
Network access appears only in the explicitly invoked model and native-library
setup scripts. This proves the exercised prerecorded runtime without network;
it does not claim that the workstation was physically disconnected.

## Stability soak

A 1,800.040-second WAV made by repeating the checksum-verified fixture was fed
through paced 20 ms delivery after all builds and short tests. The release
diagnostic completed in 1,800.643 seconds (30:00.643), performed 1,117 successful
inference passes, emitted 1,117 updates, wrote no stderr, and exited 0 after a
clean final flush.

Lightweight `/proc` sampling every 30 seconds produced 60 samples. After model
load, RSS was 333,404 KiB at the first steady sample, 333,664 KiB at the last,
and 333,664 KiB maximum: a 260 KiB observed increase over 29 minutes. Virtual
size stayed at 926,344 KiB, file descriptors stayed at three, and thread count
varied between two and five with inference activity. The final transcript was
16,383 UTF-8 bytes, within the configured 16 KiB transcript bound. No growing
event backlog or repeated fatal error was observed.

This soak continuously exercised the local streaming Whisper path and its
bounded event consumption. It did not exercise PipeWire and GTK continuously;
those paths received the shorter real-device acceptance runs above.

## Reproduction commands

Build first, then do not run Cargo concurrently with measurements:

```sh
cargo build --release --locked --workspace --all-features
sha256sum models/ggml-tiny.en.bin /tmp/lcrt-jfk.wav
ffprobe -v error -show_entries format=duration:stream=codec_name,sample_rate,channels \
  -of default=noprint_wrappers=1 /tmp/lcrt-jfk.wav
sleep 10
```

Run one excluded warm-up and then five sequential samples for each mode. Wrap
each measured invocation with the shown GNU `time` format, writing timing data
to a per-run temporary file:

```sh
/usr/bin/time \
  -f 'user_seconds=%U\nsystem_seconds=%S\ncpu_percent=%P\nmax_rss_kib=%M\nelapsed_seconds=%e\nexit_status=%x' \
  -o /tmp/lcrt-paced-1.time \
  target/release/lcrt-whisper-transcribe benchmark paced \
  models/ggml-tiny.en.bin /tmp/lcrt-jfk.wav en

/usr/bin/time \
  -f 'user_seconds=%U\nsystem_seconds=%S\ncpu_percent=%P\nmax_rss_kib=%M\nelapsed_seconds=%e\nexit_status=%x' \
  -o /tmp/lcrt-offline-1.time \
  target/release/lcrt-whisper-transcribe benchmark offline \
  models/ggml-tiny.en.bin /tmp/lcrt-jfk.wav en
```

Future engines should use the same decoded sample, language, 20 ms cumulative
pacing schedule, warm-up policy, run count, hardware state, metric definitions,
and external process measurements. A future diagnostic may have a different
binary name; no cross-backend Rust abstraction is implied by this method.

The controlled offline check was:

```sh
bwrap --unshare-net --ro-bind / / --dev-bind /dev /dev --proc /proc \
  --die-with-parent \
  target/release/lcrt-whisper-transcribe benchmark offline \
  models/ggml-tiny.en.bin /tmp/lcrt-jfk.wav en
```

## Known limitations

- This is one English speaker, one short quotation, one English-only model, and
  one Ubuntu AMD64 workstation. It is not a general accuracy evaluation.
- Paced latency begins at sample replay, not acoustic speech onset, and ends at
  backend event receipt, not visible pixels. Receipt is observed on the next 20
  ms push or final flush.
- GNU `time` CPU is rounded and process-wide. It includes model startup and, in
  paced mode, idle pacing time; it is not a per-core utilization trace.
- Five measured repetitions support min/median/max, not a meaningful p95.
- The offline transcript varied once and inference count varied from five to
  six; future comparisons must not hide equivalent outliers.
- Audible microphone transcription, same-window repeated Start/Stop, human
  visual inspection, speech-onset-to-visible-caption latency, pinned-overlay
  runtime behavior, X11, Ubuntu ARM64 hardware, and Windows remain unverified.
- The 30-minute soak covered the Whisper streaming diagnostic rather than a
  continuous integrated PipeWire/GTK session. Multi-hour stability is unknown.

## Manual multilingual follow-up (owner, 5-10 minutes)

Do not use the English-only `tiny.en` result as Vietnamese or Japanese
acceptance. For each row, choose and record a model that actually supports the
language, read the exact sentence, and fill every blank.

| Language | Reference sentence | Model used | Recognized output | Subjective usability | First visible caption latency |
| --- | --- | --- | --- | --- | --- |
| English | Real-time captions help me follow every conversation. | _owner records_ | _owner records_ | _owner records_ | _owner records_ |
| Vietnamese | Phụ đề thời gian thực giúp tôi theo dõi mọi cuộc trò chuyện. | _owner records_ | _owner records_ | _owner records_ | _owner records_ |
| Japanese | リアルタイム字幕があれば、会話を理解しやすくなります。 | _owner records_ | _owner records_ | _owner records_ | _owner records_ |

For latency, use a visible stopwatch/video if available and state the method;
otherwise record `not measured` rather than estimating. Also record the model
filename and SHA-256 alongside this table when completing it.

## Verification status

- Implemented: diagnostic-only paced/offline benchmark output; normal
  transcription and production inference behavior are unchanged.
- Unit-tested: elapsed event selection, RTF, paced chunk calculation, and JSON
  transcript escaping. The full workspace passed 87 tests before measurement.
- Runtime-tested: release benchmark runs, local network-isolated transcription,
  real microphone/system-output plumbing, GTK programmatic close, repeated
  bounded launches, and the 30-minute Whisper soak described above.
- CI-tested: pending this pull request.
- Hardware-tested: only the Ubuntu AMD64 workstation described above; no ARM64
  or Windows runtime claim is made.
