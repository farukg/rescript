#!/usr/bin/env bash
set -euo pipefail
unset BASH_ENV
mkdir -p .ci-artifacts

pacman -Syu --noconfirm --needed opam base-devel musl cmake mercurial rsync bubblewrap ninja
export OPAMYES=1 OPAMJOBS=4
opam init --bare --disable-sandboxing --yes
opam switch create rescript-5.3.0-static ocaml-variants.5.3.0+options ocaml-option-static ocaml-option-musl --yes
export OPAMSWITCH=rescript-5.3.0-static
opam install . --deps-only --yes
# Static native targets need these manifest-pinned test libraries, not the browser toolchain.
opam install --switch=rescript-5.3.0-static ounit2.2.2.7 yojson.2.2.2 --yes
opam install --switch=rescript-5.3.0-static dune.3.21.0 --yes

rust_version=$(node -p 'require("node:fs").readFileSync("rewatch/Cargo.toml", "utf8").match(/^rust-version = "([^"]+)"/m)[1]')
rustup toolchain install "$rust_version" --profile minimal --component clippy --target x86_64-unknown-linux-musl
export RUSTUP_TOOLCHAIN="$rust_version"
export CARGO_TARGET_DIR="$PWD/rewatch/target"
node .yarn/releases/yarn-4.10.3.cjs install --immutable
bash scripts/ci/buildStaticCompiler.sh
cargo build --manifest-path rewatch/Cargo.toml --release --target x86_64-unknown-linux-musl
mkdir -p rewatch/target/release
cp rewatch/target/x86_64-unknown-linux-musl/release/rescript rewatch/target/release/rescript
node scripts/copyExes.js --rewatch
LDFLAGS=-static node scripts/buildNinjaBinary.js
node scripts/copyExes.js --ninja
node .yarn/releases/yarn-4.10.3.cjs workspace @rescript/runtime build
git diff --exit-code -- packages/@rescript/runtime/lib/js packages/@rescript/runtime/lib/es6
node scripts/ci/testRewatch.cjs
cargo clippy --manifest-path rewatch/Cargo.toml --all-targets --all-features
npm run test-installation
node scripts/preparePackage.cjs
node .yarn/releases/yarn-4.10.3.cjs exec node scripts/updateArtifactList.js
git diff --exit-code -- packages/artifacts.json
printf '%s\n' "$CI_COMMIT_SHA" > .ci-artifacts/source-sha.txt
sha256sum packages/@rescript/linux-x64/bin/*.exe > .ci-artifacts/toolchain.sha256
npm pack --pack-destination .ci-artifacts
