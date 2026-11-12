#!/usr/bin/env bash
# Runs the tests, builds a release Flipclock and copies it into a folder ready to use.
#
#   ./publish.sh                    # tests, then publishes to ./publish
#   ./publish.sh -o ~/Flipclock     # anywhere else
#   ./publish.sh --clean            # empty the folder first
#   ./publish.sh --skip-tests
#
# Linux: the folder gets the `flipclock` binary.
# macOS: the folder gets `Flipclock.app` (built with cargo-bundle).
# An existing config.toml in the folder is always kept, so publishing over an
# installed copy keeps its settings.
set -euo pipefail

cd "$(dirname "$0")"
output="$PWD/publish"
clean=0
skip_tests=0

while [ $# -gt 0 ]; do
  case "$1" in
    -o|--output) output="$2"; shift 2 ;;
    --clean) clean=1; shift ;;
    --skip-tests) skip_tests=1; shift ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
done

if [ "$skip_tests" -eq 0 ]; then
  echo "==> Running the tests"
  cargo test --workspace
fi

mkdir -p "$output"
if [ "$clean" -eq 1 ]; then
  find "$output" -mindepth 1 -maxdepth 1 ! -name config.toml -exec rm -rf {} +
fi

case "$(uname -s)" in
  Darwin)
    if ! command -v cargo-bundle >/dev/null 2>&1; then
      echo "cargo-bundle is missing: run 'cargo install cargo-bundle' first" >&2
      exit 1
    fi
    echo "==> Building Flipclock.app"
    (cd crates/flipclock-app && cargo bundle --release)
    rm -rf "$output/Flipclock.app"
    cp -R target/release/bundle/osx/Flipclock.app "$output/"
    ;;
  *)
    echo "==> Building the release"
    cargo build --release -p flipclock-app
    cp target/release/flipclock "$output/"
    ;;
esac

echo "==> Published to $output"
if [ -f "$output/config.toml" ]; then
  echo "    (existing config.toml kept)"
fi
