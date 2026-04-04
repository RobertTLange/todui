# Development

## Source Build

```bash
cargo install --path .
todui --help
```

## Verification Gate

Rust:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

npm wrapper:

```bash
TODOUI_SKIP_DOWNLOAD=1 npm ci
npm run build
npm test
```

## Local Launcher Smoke Test

Build the local Rust binary, then point the npm wrapper at it:

```bash
cargo build --locked
TODOUI_BINARY_PATH="$PWD/target/debug/todui" node bin/todui.js --help
```

To validate the packed npm artifact locally:

```bash
TARBALL="$(npm pack | tail -n 1)"
PREFIX_DIR="$(mktemp -d)"
TODOUI_BINARY_PATH="$PWD/target/debug/todui" npm install -g --prefix "$PREFIX_DIR" "./$TARBALL"
"$PREFIX_DIR/bin/todui" --help
```

## Release Workflow

`todui` uses a manual GitHub Actions release modeled after `agentlens`.

Requirements:

- run the workflow from `main`
- keep `Cargo.toml` and `package.json` on the same version
- ensure the target version exists in `CHANGELOG.md`
- provide `NPM_TOKEN` in repo secrets

The workflow:

1. runs the Rust and npm verification gates
2. rejects existing tags and already-published npm versions
3. builds prebuilt binaries for:
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
4. publishes `@roberttlange/todui` to npm with provenance
5. creates a GitHub release and uploads the binaries plus `.sha256` files
