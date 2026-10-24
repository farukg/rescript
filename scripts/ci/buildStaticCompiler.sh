#!/usr/bin/env bash
set -euo pipefail

opam exec -- dune build --profile static
node scripts/copyExes.js --compiler
