#!/usr/bin/env bash

set -euo pipefail

yarn prettier --write tests
cargo fmt
