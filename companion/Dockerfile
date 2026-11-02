FROM rust:1.99-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p x3-companion
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home x3 && mkdir /data && chown x3:x3 /data
COPY --from=build /src/target/release/x3-companion /usr/local/bin/x3-companion
USER 10001:10001
ENV X3_DATA_DIR=/data X3_PUBLIC_BIND=0.0.0.0:8787 X3_DEVICE_BIND=0.0.0.0:8788
VOLUME ["/data"]
EXPOSE 8787 8788
ENTRYPOINT ["x3-companion"]
CMD ["serve"]
