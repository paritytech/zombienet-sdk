# build stage
FROM rust:1.75-alpine as builder

WORKDIR /tmp

COPY . .

RUN apk add musl-dev

RUN cargo build --release -p zombienet-file-server

# run stage
FROM alpine:latest

ENV LISTENING_ADDRESS 0.0.0.0:80
ENV UPLOADS_DIRECTORY /uploads

COPY --from=builder /tmp/target/release/zombienet-file-server /usr/local/bin/file-server

CMD ["file-server"]