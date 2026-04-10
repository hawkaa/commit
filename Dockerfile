FROM rust:1.94-alpine AS builder

RUN apk add --no-cache musl-dev gcc make pkgconfig lld

ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld"

WORKDIR /build

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Create dummy sources to build deps
RUN mkdir -p src/bin && \
    echo "pub fn dummy() {}" > src/lib.rs && \
    echo "fn main() {}" > src/main.rs && \
    echo "fn main() {}" > src/bin/mcp.rs
RUN cargo build --release --bin commit-backend 2>/dev/null ; true

# Copy actual source
COPY src/ src/
COPY tests/ tests/

# Touch sources to invalidate cache and rebuild
RUN find src -name "*.rs" -exec touch {} +
RUN cargo build --release --bin commit-backend

FROM scratch

COPY --from=alpine:latest /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=alpine:latest /etc/passwd /etc/passwd
COPY --from=alpine:latest /etc/group /etc/group

COPY --from=busybox:uclibc /bin/busybox /bin/busybox
RUN ["/bin/busybox", "--install", "-s", "/bin/"]

COPY --from=builder /build/target/release/commit-backend /usr/local/bin/commit-backend

RUN ["/bin/busybox", "mkdir", "-p", "/data", "/tmp"]

ENV DATABASE_PATH=/data/commit.db
EXPOSE 3000

USER nobody
CMD ["commit-backend"]
