#!/bin/sh
set -eu

model_dir=${1:-models}
model_name=ggml-tiny.en.bin
model_url=https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin
model_sha256=921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f

mkdir -p "$model_dir"
curl --fail --location --progress-bar --output "$model_dir/$model_name.part" "$model_url"
printf '%s  %s\n' "$model_sha256" "$model_dir/$model_name.part" | sha256sum --check --status
mv "$model_dir/$model_name.part" "$model_dir/$model_name"
printf '%s\n' "$model_dir/$model_name"
