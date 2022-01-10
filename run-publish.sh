#!/usr/bin/env bash

set -e -o pipefail
anchor build
cp ./target/types/voter_stake_registry.ts src/idl.ts
yarn clean && yarn build && cp package.json ./dist/ && yarn publish dist

