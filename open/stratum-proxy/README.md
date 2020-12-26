# Overview

This proxy demonstrates 1:1 translation of Stratum V2 protocol to Stratum V1. It
listens for downstream Stratum V2 connections and translates the protocol to
upstream Stratum V1 node.


# Build

## Prerequisites

- Rust toolchain installed via [rustup](https://rustup.rs/)

## Building it

`cargo build --release`

## Running the proxy via cargo

This command builds the proxy, too:

`cargo run --release -- --listen 0.0.0.0:3333 --v1-upstream stratum.slushpool.com:3333 --certificate-file server-noise-static-public.cert`

## Running it directly

`./target/release/ii-stratum-proxy --listen 0.0.0.0:3333 --v1-upstream stratum.slushpool.com:3333 --certificate-file server-noise-static-public.cert`



# Future Work

Below is a high level list of areas that still need to be resolved:

- handle multiple channels on a single downstream connection
- use V2 submission sequence numbers for batch acknowledgement of valid job
  solutions
- improve logging
- resolve all TODO's in the sources
