FROM ghcr.io/tlsnotary/tlsn/notary-server:v0.1.0-alpha.12

COPY config.yaml /root/.notary/config.yaml
COPY entrypoint.sh /root/.notary/entrypoint.sh

ENTRYPOINT ["/bin/sh", "/root/.notary/entrypoint.sh"]
