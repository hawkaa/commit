#!/bin/sh
set -eu

if [ -z "${NOTARY_SIGNING_KEY:-}" ]; then
  echo "FATAL: NOTARY_SIGNING_KEY not set" >&2
  exit 1
fi

# Write the signing key from the Fly secret to a file.
# printf preserves newlines from the env var (PEM format).
printf '%s\n' "$NOTARY_SIGNING_KEY" > /root/.notary/notary.key
chmod 600 /root/.notary/notary.key

exec notary-server --config /root/.notary/config.yaml
