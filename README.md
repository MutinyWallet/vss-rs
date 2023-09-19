# vss-rs

A rust implementation of a VSS server.

## Usage

You need a postgres database and an authentication key. These can be set in the environment variables `DATABASE_URL`
and `AUTH_KEY` respectively. This can be set in a `.env` file in the root of the project.

To run the server, run `cargo run --release` in the root of the project.
