# syntax=docker/dockerfile:1

# ---- 빌드 단계 ----------------------------------------------------------------
# 실행 이미지(bookworm, glibc 2.36)와 같은 배포판으로 빌드해 glibc 버전 불일치를 막는다
FROM rust:1-slim-bookworm AS builder
WORKDIR /src

COPY . .

# 레지스트리·target 을 BuildKit 캐시 마운트로 유지해 재빌드 시 의존성을 다시 컴파일하지 않는다.
# 캐시 마운트는 이미지에 남지 않으므로 바이너리는 같은 RUN 안에서 밖으로 복사한다.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,id=netsci-target-bookworm,target=/src/target \
    cargo build --release --locked -p netsci \
    && cp target/release/netsci /usr/local/bin/netsci

# ---- 실행 단계 ----------------------------------------------------------------
FROM debian:bookworm-slim

# HTTPS(rustls) 인증서 검증용 루트 인증서
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --uid 10001 --user-group --home-dir /app --shell /usr/sbin/nologin netsci \
    && mkdir -p /app/data \
    && chown -R netsci:netsci /app

COPY --from=builder /usr/local/bin/netsci /usr/local/bin/netsci

WORKDIR /app
USER netsci
VOLUME /app/data
ENTRYPOINT ["netsci"]
