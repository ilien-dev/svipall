"""Export the speech recogniser `web_video` uses when a video has no captions.

Whisper base, multilingual (MIT), exported to ONNX and quantised to int8, plus the two small files
the Rust side needs to use it. No account, no key, no service: the weights are downloaded once from
their public release and everything else is local.

    uv run --python 3.12 --with optimum-onnx --with onnx --with onnxruntime --with transformers \
        --with torch tools/models/export_asr.py [--out ~/.svipall/models] [--fixtures crates/svipall/tests/fixtures/asr]

What comes out, and the contract each keeps with `docs/models.md`:

  asr_encoder.onnx   input `input_features` [1, n_mels, 3000] -> `last_hidden_state`
  asr_decoder.onnx   inputs `input_ids` [1, n] and `encoder_hidden_states` -> `logits` [1, n, vocab]
                     (no cache: the whole prefix is fed each step, which keeps the Rust side to one
                     session call per token and costs little at this size)
  asr.json           the front end (mel bins, FFT, hop, rate, window length), the special tokens
                     and the language tokens, so nothing about the model is written in Rust
  asr_vocab.json     token id -> byte-level BPE piece, for turning ids back into text

With `--fixtures`, it also writes the reference log-mel of a fixed synthetic signal, which the Rust
front end is tested against: two implementations of one formula agree, or a test says where not.
"""

import argparse
import json
import math
import pathlib
import tempfile

import numpy as np
from onnxruntime.quantization import QuantType, quantize_dynamic
from optimum.exporters.onnx import main_export
from transformers import WhisperFeatureExtractor, WhisperTokenizer

MODEL = "openai/whisper-base"


def signal(rate: int) -> np.ndarray:
    """One second of two tones and a sweep: the fixture both sides compute from scratch."""
    t = np.arange(rate, dtype=np.float64) / rate
    x = 0.4 * np.sin(2 * math.pi * 440 * t) + 0.2 * np.sin(2 * math.pi * 1000 * t)
    x += 0.1 * np.sin(2 * math.pi * (200 + 1800 * t) * t)
    return x.astype(np.float32)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(pathlib.Path.home() / ".svipall" / "models"))
    ap.add_argument("--fixtures", default=None)
    args = ap.parse_args()
    out = pathlib.Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory() as tmp:
        main_export(MODEL, output=tmp, task="automatic-speech-recognition", no_post_process=True)
        for src, dst in [("encoder_model.onnx", "asr_encoder.onnx"), ("decoder_model.onnx", "asr_decoder.onnx")]:
            quantize_dynamic(
                str(pathlib.Path(tmp) / src),
                str(out / dst),
                weight_type=QuantType.QInt8,
            )

    fe = WhisperFeatureExtractor.from_pretrained(MODEL)
    tok = WhisperTokenizer.from_pretrained(MODEL)
    special = lambda s: tok.convert_tokens_to_ids(s)
    # `<|en|>`, `<|es|>`, `<|haw|>`: two or three lowercase letters is a language, every other
    # special token is longer or not a word.
    languages = {
        t[2:-2]: special(t)
        for t in tok.additional_special_tokens
        if len(t) in (6, 7) and t[2:-2].isalpha() and t[2:-2].islower()
    }
    timestamp_begin = special("<|notimestamps|>") + 1
    sidecar = {
        "n_mels": fe.feature_size,
        "n_fft": fe.n_fft,
        "hop": fe.hop_length,
        "sample_rate": fe.sampling_rate,
        "chunk_seconds": fe.chunk_length,
        "frames": fe.nb_max_frames,
        "time_precision": 0.02,
        "max_initial_timestamp": 1.0,
        "tokens": {
            "eot": tok.eos_token_id,
            "sot": special("<|startoftranscript|>"),
            "transcribe": special("<|transcribe|>"),
            "translate": special("<|translate|>"),
            "no_timestamps": special("<|notimestamps|>"),
            "no_speech": special("<|nospeech|>") if "<|nospeech|>" in tok.get_vocab() else special("<|nocaptions|>"),
            "timestamp_begin": timestamp_begin,
        },
        "languages": languages,
        # The mel filterbank as the feature extractor builds it, so the Rust side multiplies by the
        # same matrix instead of re-deriving a formula two libraries disagree on at the edges.
        "mel_filters": np.asarray(fe.mel_filters, dtype=np.float32).T.round(8).tolist(),
    }
    (out / "asr.json").write_text(json.dumps(sidecar))
    vocab = tok.convert_ids_to_tokens(list(range(timestamp_begin)))
    (out / "asr_vocab.json").write_text(json.dumps(vocab, ensure_ascii=False))

    if args.fixtures:
        fx = pathlib.Path(args.fixtures)
        fx.mkdir(parents=True, exist_ok=True)
        feats = fe(signal(fe.sampling_rate), sampling_rate=fe.sampling_rate, return_tensors="np")
        mel = feats["input_features"][0]  # [n_mels, 3000]
        ref = {
            "signal": "0.4 sin(440) + 0.2 sin(1000) + 0.1 sin(sweep 200..2000), 1 s at 16 kHz",
            "frames": [[round(float(v), 5) for v in mel[:, f]] for f in (0, 10, 50, 99, 100, 2999)],
            "frame_index": [0, 10, 50, 99, 100, 2999],
        }
        (fx / "mel_reference.json").write_text(json.dumps(ref))
        # The front end alone (no vocabulary, no languages), which is what the parity test reads.
        front = {k: v for k, v in sidecar.items() if k != "languages"}
        (fx / "asr_frontend.json").write_text(json.dumps(front))
    for f in sorted(out.glob("asr*")):
        print(f"{f.name}\t{f.stat().st_size}")


if __name__ == "__main__":
    main()
