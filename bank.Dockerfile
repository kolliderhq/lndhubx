FROM rustlang/rust:nightly as builder

WORKDIR /app
COPY . .
RUN apt-get update && apt-get install -y cmake libpq-dev libzmq3-dev
RUN cd /app && cargo build --bin bank --release --target-dir .

FROM ubuntu:20.04
WORKDIR /app
RUN apt-get update && apt-get install -y cmake libpq-dev libzmq3-dev
ENV TINI_VERSION v0.19.0
ADD https://github.com/krallin/tini/releases/download/${TINI_VERSION}/tini /tini
RUN chmod +x /tini
ENTRYPOINT ["/tini", "-g", "--"]
COPY --from=builder /app /app
RUN mv /app/release/bank /app/bank-bin
ENV ENV dev
ENV FILE_NAME lndhubx
CMD ["sh", "-c", "/app/bank-bin"]
