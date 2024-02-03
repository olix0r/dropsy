FROM docker.io/library/rust:1.75-slim-bookworm as builder
COPY . /usr/src/dropsy
WORKDIR /usr/src/dropsy
RUN cargo fetch
RUN cargo build --release

FROM docker.io/library/debian:bookworm-slim
COPY --from=builder /usr/src/dropsy/target/release/dropsy /usr/local/bin/dropsy
CMD ["/usr/local/bin/dropsy"]
