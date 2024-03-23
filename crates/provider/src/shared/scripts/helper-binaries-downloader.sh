#!/bin/ash

log() {
    echo "$(date +"%F %T") $1"
}

wget github.com/moparisthebest/static-curl/releases/download/v7.83.1/curl-amd64 -O /cfg/curl
log "curl downloaded"

chmod +x /cfg/curl
log "curl chmoded"

wget -qO- github.com/uutils/coreutils/releases/download/0.0.17/coreutils-0.0.17-x86_64-unknown-linux-musl.tar.gz | tar -xz -C /cfg --strip-components=1 coreutils-0.0.17-x86_64-unknown-linux-musl/coreutils
log "coreutils downloaded"

chmod +x /cfg/coreutils
log "coreutils chmoded"