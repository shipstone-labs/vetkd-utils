#!/usr/bin/env bash
set -e  # Exit on error
echo "Deploying to ${NETWORK}"
if dfx canister id vetkd_notes --network $NETWORK 2>&1 > /dev/null; then
  echo "vetkd_notes already exists"
else
  dfx canister create vetkd_notes --network $NETWORK --identity $IDENTITY
fi
if dfx canister id vetkd_system_api --network $NETWORK 2>&1 > /dev/null; then
  echo "vetkd_system_api already exists"
else
  dfx canister create vetkd_system_api --network $NETWORK --identity $IDENTITY
fi
if dfx canister id vetkd_www --network $NETWORK 2>&1 > /dev/null; then
  echo "vetkd_www already exists"
else
  dfx canister create vetkd_www --network $NETWORK --identity $IDENTITY
fi
if dfx canister id internet_identity --network $NETWORK 2>&1 > /dev/null; then
  echo "internet_identity already exists"
else
  dfx canister create internet_identity --network $NETWORK --identity $IDENTITY
fi

touch ./packages/vetkd-notes-canister/src/lib.rs
VETKD_SYSTEM_API_CANISTER_ID=$(dfx canister id vetkd_system_api --network $NETWORK) dfx build vetkd_notes --network $NETWORK
LOCAL_HASH=$(sha256sum .dfx/$NETWORK/canisters/vetkd_notes/vetkd_notes.wasm | awk '{ print "0x" $1 }')
REMOTE_HASH=$(dfx canister info vetkd_notes --network $NETWORK | grep 'Module hash' | awk '{ print $3 }')
if [ "$LOCAL_HASH" != "$REMOTE_HASH" ]; then
  VETKD_SYSTEM_API_CANISTER_ID=$(dfx canister id vetkd_system_api --network $NETWORK) dfx deploy vetkd_notes --network $NETWORK --identity $IDENTITY
else
  echo "vetkd_notes is up to date"
fi
VETKD_SYSTEM_API_CANISTER_ID=$(dfx canister id vetkd_system_api --network $NETWORK) dfx generate vetkd_notes --network $NETWORK --identity $IDENTITY

LOCAL_HASH=$(sha256sum .dfx/$NETWORK/canisters/vetkd_system_api/vetkd_system_api.wasm | awk '{ print "0x" $1 }')
REMOTE_HASH=$(dfx canister info vetkd_system_api --network $NETWORK | grep 'Module hash' | awk '{ print $3 }')
if [ "$LOCAL_HASH" != "$REMOTE_HASH" ]; then
  dfx deploy vetkd_system_api --network $NETWORK --identity $IDENTITY
else
  echo "vetkd_system_api is up to date"
fi

LOCAL_HASH=$(curl https://github.com/dfinity/internet-identity/releases/latest/download/internet_identity_dev.wasm.gz | gzip -d | sha256sum | awk '{ print "0x" $1 }')
REMOTE_HASH=$(dfx canister info internet_identity --network $NETWORK | grep 'Module hash' | awk '{ print $3 }')
if [ "$LOCAL_HASH" != "$REMOTE_HASH" ]; then
  dfx deploy vetkd_system_api --network $NETWORK --identity $IDENTITY
else
  echo "vetkd_system_api is up to date"
fi

touch ./packages/frontend/src/main.ts
dfx deploy vetkd_www --network $NETWORK --identity $IDENTITY
