# vss-rs

A rust implementation of a VSS server. Based on [LDK's `vss-server` reference implementation](https://github.com/lightningdevkit/vss-server). See the [API Contract](https://github.com/lightningdevkit/vss-server/blob/main/app/src/main/proto/vss.proto).

## Usage

You need a postgres database and an authentication key. These can be set in the environment variables `DATABASE_URL`
and `AUTH_KEY` respectively. These can be set in a `.env` file in the root of the project. If you do not have an
authentication key, leave this unset and the server will skip authentication.

To run the server, run `cargo run --release` in the root of the project.

## Configuration

vss-rs is configured via environment variables, which may be set in an `.env` file in the working directory, or injected dynamically (command-line prefix, container orchestration, etc.) See `.env.sample`.

 - `DATABASE_URL`: a postgres connection string of the format `postgres://u:p@host[:port]/dbname`
 - `VSS_PORT`: (optional; default 8080) host port to bind
 - `AUTH_KEY`: (optional; default none) hex-encoded ES256K public key
 - `SELF_HOST`: (optional; default false)
 - `ADMIN_KEY`: (optional; default none) key to use as bearer token to trigger admin actions like migration

## Database

Scheme migrations can be run manually via `diesel-cli`, or automatically on startup when `SELF_HOST` is true.

They can also be triggered _ad hoc_ by passing a bearer token corresponding to `ADMIN_KEY` to the `/migrations` endpoint.

## CORS

CORS headers are supplied with responses, and Origin headers are validated against the list when handling requests. This behavior is disabled when `SELF_HOST` is true.

If you intend to host this in a public-facing way (_i.e._, not just on `localhost`), you'll need to add your domain to the `ALLOWED_ORIGINS` in `main.rs`.

## Authentication

In production usage, the VSS clients (lightning wallets) should authenticate with a [JSON Web Token(JWT)](https://datatracker.ietf.org/doc/html/rfc7519) issued by an identity provider (not included in VSS-RS). 

### Authentication Key

The authentication key, set with `AUTH_KEY`, is a hex-encoded ECDSA _public_ key on the p256k1 curve and is used to validate the signature on a client-supplied JWT. The VSS client may have obtained the JWT from any issuing party as long as you set the appropriate public key here. The JWT should have set the `alg` parameter to `ES256K`. This is uncommon and should not be confused with `ES256`.