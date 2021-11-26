#!/usr/bin/env bash

set -euo pipefail

./run-format.sh && anchor test -- --features localnet