#!/usr/bin/env bash
# Build ONNX Runtime from source, statically, and print the directory to point `ORT_LIB_LOCATION`
# at. One script for every caller, so the version and the flags cannot drift between the workflow
# that warms the cache and the one that consumes it.
#
#   tools/onnxruntime/build.sh [build-dir] [arch]      # defaults: $PWD/.ort, this machine's arch
#
# Why this exists: `ort` downloads prebuilt ONNX Runtime binaries, and those reference glibc 2.38
# and GCC 13's libstdc++, so a Linux artefact using them starts only on Ubuntu 24.04 and newer —
# and none is published for x86-64 macOS at the pinned version at all. Building it here means the
# runtime links whatever glibc the runner has, which for the 22.04 image is 2.35, and every target
# can carry the captcha models instead of some of them.
#
# `ORT_LIB_LOCATION` is read by ort-sys before its download path, so nothing in Cargo.toml changes.
# It must point at the directory that *contains* the `libonnxruntime_*.a` files and their `_deps`,
# which is `<build-dir>/Release` — one level below what ONNX Runtime calls its build directory.
#
# The version is not free to choose. `ort 2.0.0-rc.13` asks for ONNX Runtime API 27, and a runtime
# older than that links perfectly and then refuses at the first session with `The requested API
# version [27] is not available` — which `svipall doctor` cannot see, because the models are
# embedded and listed either way. `crates/svipall/tests/models.rs` is what catches it: run those
# tests against any build produced this way before believing it works.
#
# One more trap, for anyone reusing a target directory: this path does not change when the runtime
# behind it does, and cargo only re-runs a build script when an environment variable's *value*
# changes. After bumping VERSION, `cargo clean -p ort-sys -p ort`, or the next binary links the old
# runtime — and it says so at the first session, never at build time.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
version="$(tr -d '[:space:]' < "$here/VERSION")"
build_dir="${1:-$PWD/.ort}"
# The architecture matters on macOS and nowhere else: GitHub's `macos-latest` is arm64, and the
# x86-64 macOS artefact is cross-compiled on it. Rust does that from the target triple; a cmake
# build does not, so without this the Intel artefact would link an arm64 runtime and fail at the
# link step with a pile of unresolved symbols. On Linux cross-building the runtime is not supported
# here — CI uses a native arm runner — so a mismatch is refused rather than half-attempted.
arch="${2:-$(uname -m)}"
case "$arch" in amd64|x64) arch=x86_64 ;; aarch64) [ "$(uname -s)" = Darwin ] && arch=arm64 ;; esac
if [ "$(uname -s)" != Darwin ] && [ "$arch" != "$(uname -m)" ]; then
    echo "cannot build the runtime for $arch on a $(uname -m) $(uname -s)" >&2
    exit 1
fi
src_dir="$build_dir/src"
out_dir="$build_dir/build/Release"

if [ -f "$out_dir/libonnxruntime_common.a" ]; then
    echo "onnxruntime $version already built in $out_dir" >&2
    echo "$out_dir"
    exit 0
fi

# ONNX Runtime wants a newer cmake than Ubuntu 22.04 ships (3.22), and ninja is measurably faster
# than make here. pip has both, and this way macOS and Linux take the same path.
python3 -m pip install --quiet --upgrade cmake ninja

# `build.py` reads CMAKE_OSX_ARCHITECTURES through `add_default_definition`, so an explicit one
# wins. Unquoted below on purpose: it is empty on Linux and must disappear rather than become an
# empty argument.
osx_defines=""
if [ "$(uname -s)" = Darwin ]; then
    osx_defines="CMAKE_OSX_ARCHITECTURES=$arch"
fi

mkdir -p "$build_dir"
rm -rf "$src_dir"
git clone --depth 1 --branch "v$version" --recursive --shallow-submodules \
    https://github.com/microsoft/onnxruntime "$src_dir"

# No shared library, no tests, and position-independent code because it is linked into a Rust
# binary. Everything else is ONNX Runtime's own default CPU build.
"$src_dir/build.sh" \
    --config Release \
    --parallel "$(getconf _NPROCESSORS_ONLN)" \
    --build_dir "$build_dir/build" \
    --skip_tests \
    --skip_submodule_sync \
    --allow_running_as_root \
    --compile_no_warning_as_error \
    --cmake_extra_defines \
        onnxruntime_BUILD_SHARED_LIB=OFF \
        CMAKE_POSITION_INDEPENDENT_CODE=ON \
        onnxruntime_BUILD_UNIT_TESTS=OFF \
        $osx_defines

# `re2` is configured by ONNX Runtime and then not built, because with unit tests off nothing in
# the default target set links it — and ort-sys links it unconditionally, so without this the Rust
# build ends in `could not find native static library re2`. Twenty seconds, and the alternative is
# leaving that error for whoever next points ORT_LIB_LOCATION at a source build.
cmake --build "$build_dir/build/Release" --target re2 -j "$(getconf _NPROCESSORS_ONLN)"

# The source tree is 600 MB of checkout that nothing after this needs, and it is the difference
# between a cache entry that is worth keeping and one that is mostly git history.
rm -rf "$src_dir"

[ -f "$out_dir/libonnxruntime_common.a" ] || {
    echo "the build finished but $out_dir/libonnxruntime_common.a is not there" >&2
    exit 1
}

# ONNX Runtime occasionally moves code into a static library of its own, and ort-sys links a fixed
# list it knows nothing about: 1.28 split out `model_package/libmodel_package.a`, which
# `libonnxruntime_session.a` references, so a debug build fails with `undefined symbol:
# ModelPackage_Open` while a release build happens to survive because `--gc-sections` drops the
# path. Anything of that shape found here is written out as linker flags for the caller to append
# to RUSTFLAGS, so a version bump does not need this script rewritten.
extra="$build_dir/extra-rustflags"
: > "$extra"
for dir in "$out_dir"/*/; do
    name="$(basename "$dir")"
    lib="$dir/lib$name.a"
    [ -f "$lib" ] || continue
    printf ' -L native=%s -l static=%s' "${dir%/}" "$name" >> "$extra"
    echo "extra static library: $lib" >&2
done

echo "$out_dir"
