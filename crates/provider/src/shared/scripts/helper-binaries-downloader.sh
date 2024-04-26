#!/bin/ash

log() {
    echo "$(date +"%F %T") $1"
}

# used to handle the distinction where /cfg is used for k8s and /helpers for docker/podman
# to share a volume across nodes containing helper binaries and independent from /cfg
# where some node files are stored
OUTDIR=$([ -d /helpers ] && echo "/helpers" || echo "/cfg")

wget github.com/moparisthebest/static-curl/releases/download/v7.83.1/curl-amd64 -O "$OUTDIR/curl"
log "curl downloaded"

chmod +x "$OUTDIR/curl"
log "curl chmoded"

wget -qO- github.com/uutils/coreutils/releases/download/0.0.17/coreutils-0.0.17-x86_64-unknown-linux-musl.tar.gz | tar -xz -C $OUTDIR --strip-components=1 coreutils-0.0.17-x86_64-unknown-linux-musl/coreutils
log "coreutils downloaded"

chmod +x "$OUTDIR/coreutils"
log "coreutils chmoded"