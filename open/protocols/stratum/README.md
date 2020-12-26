# Overview

This is a Stratum protocol software package that provides:

- Stratum V1/V2 primitives implemented in Rust
- [Simulator](sim/README.md) used to verify the design of Stratum V2

## Stratum Server Certificate Workflow

Stratum V2 security is based on [noise handshake](https://noiseprotocol.org/noise.html#handshake-patterns), where the static public key of the stratum server is signed by a simple **Certification Authority**. Any stratum client is required to have preconfigured the public key of this certification authority and use it to verify the authenticity of the static public key presented by the server.

### Building the Tool

For security reasons, we recommend using the tool built from sources.

Setup the Rust toolchain installed via [rustup](https://rustup.rs/).

### Workflow
The overall workflow requires:

#### Generating a **Certification Authority ED25519 keypair**

 This **CA keypair** *MUST* never be deployed to a stratum server and is used
 exclusively for signing the server keys:

 ```
 cargo run -- gen-ca-key
 ```

The resulting keys are in:
  - `ca-ed25519-public.key`, and
  - `ca-ed25519-secret.key`.

Keep them safe!

#### Generating server keypair

```
cargo run -- gen-noise-key
```

The resulting keys are in:

 - `server-noise-static-secret.key`, and
 - `server-noise-static-public.key`.

#### Signing the server public key

 We will sign the public key from the previous step with the **CA Private Key** and produce a certificate that has a specified validity (defaults to 90 days).

```
cargo run -- sign-key --public-key-to-sign server-noise-static-public.key
 --signing-key ca-ed25519-secret.key
```

The following files are to be uploaded to the stratum server that provides
 stratum v2 (e.g. our ii-stratum-proxy):

 - `server-noise-static-secret.key`, and
 - `server-noise-static-public.cert`.

#### Testing Stratum Client Setup

In case you decide to run the miner against your own stratum V2 endpoint (e.g. [ii-stratum-proxy](../../stratum-proxy/README.md)) you have to pass it the actual public key of the Pool CA that has been used for signing.


## Running Protocol Test suite

`cargo test --all`
