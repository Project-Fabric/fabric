#!/bin/sh
# Creates the ledger and role credentials on first start (existing ones are kept), then runs
# the given cosigna-agent command against the /data volume.
set -eu
umask 077
cosigna-agent init --data-dir /data >/dev/null
exec cosigna-agent "$@" --data-dir /data
