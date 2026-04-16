# cargo-rustflags

Resolve effective RUSTFLAGS for a cargo target.

## How it works

`cargo-rustflags` uses cargo's own config resolution by running `cargo check`
with RUSTC_WRAPPER set to itself.

## Usage: 

`cargo rustflags [--target ...] [--config ...] [--list] [--encoded]`

- `--target`: Select a specific target (default: current target).
- `--config`: Load a config file (default: none).
- `--list`: Print RUSTFLAGS one-per-line.
- `--encoded`: Print the encoded RUSTFLAGS (ie: CARGO_ENCODED_RUSTFLAGS).

## Examples:

```sh
# Print the effective RUSTFLAGS for the current target
$ cargo rustflags
-Copt-level=2 -Wunused

# Print the effective RUSTFLAGS for the specific target
$ cargo rustflags --target x86_64-unknown-linux-gnu
-Copt-level=2 -Wunused

# Print the effective RUSTFLAGS for the target, using a config file
$ cargo rustflags --config .cargo/other.toml \
    --config 'target.x86_64-unknown-linux-gnu.rustflags=["-Clink-arg=-fuse-ld=lld"]'
-Clink-arg=-fuse-ld=lld -Wunused

# Print RUSTFLAGS one-per-line
$ cargo rustflags --list
-Copt-level=2
-Wunused

# Print the encoded RUSTFLAGS for the target (ie: CARGO_ENCODED_RUSTFLAGS)
$ cargo rustflags --encoded --target x86_64-unknown-linux-gnu
-Clink-arg=-fuse-ld=lld\x1f-Copt-level=2\x1f-Wunused
```
