# cargo-rustflags

Resolve effective RUSTFLAGS for a cargo target.

## Usage: 

`cargo rustflags [--target ...] [--config ...]`

## Examples:

```
cargo rustflags --target x86_64-unknown-linux-gnu
cargo rustflags --config .cargo/other.toml --config 'target.x86_64-unknown-linux-gnu.rustflags=["-Clink-arg=-fuse-ld=lld"]'
```
