# Install

SyncFleet is a native Rust executable plus a thin Python launcher. The launcher
does **not** reimplement the engine; it finds and runs the native binary.

## Native CLI (recommended)

Requires a stable Rust toolchain (MSRV **1.89**).

```bash
git clone ../SyncFleet
cd SyncFleet
cargo build --release
export PATH="$PWD/target/release:$PATH"
syncfleet --version
syncfleet --help
```

## Python wrapper (PyPI)

```bash
pip install syncfleet
syncfleet --help
```

The wrapper locates the native executable in this order:

1. `SYNCFLEET_BIN` — explicit path (highest priority);
2. a binary bundled in the installed wheel (`syncfleet/bin/`);
3. `syncfleet` on `PATH` (skipping the wrapper itself).

If none is found it exits `127` with a clear message.

```bash
export SYNCFLEET_BIN=/usr/local/bin/syncfleet
```

### Platform matrix (PyPI 0.7.0)

| Platform | PyPI wheel (bundled binary) |
|---|---|
| macOS (universal2) | ✅ `py3-none-macosx_10_9_universal2` |
| Linux x86_64 | build from source (or use sdist + native binary) |
| Windows x86_64 | build from source |
| macOS Intel | covered by the universal2 wheel |

The release workflow defines Linux, macOS Intel/ARM, and Windows wheels, but
**only the macOS universal2 wheel is published for 0.7.0**. On other platforms,
install the native CLI from source and point `SYNCFLEET_BIN` at it.

## Requirements

- Rust 1.89+ (source builds)
- Python 3.8+ (only for the wrapper)
- A local SQLite database per node
- Nodes reachable over TCP; mutual TLS 1.3; trusted LAN/VPN recommended

## Verify

```bash
syncfleet --version
syncfleet status
syncfleet doctor --full
```

## Troubleshooting

- **`syncfleet: command not found`** — the native binary is not on `PATH`; build
  from source or set `SYNCFLEET_BIN`.
- **Wrapper exits 127** — no bundled binary and no `PATH`/`SYNCFLEET_BIN`
  executable.
- **`syncfleet --version` prints an older version** — you are running a stale
  binary from `target/release`; rebuild with `cargo build --release`.
- **TLS/peering failures** — both peers must be enrolled and have pinned each
  other's certificates; see the README enrollment steps and
  [Security](SECURITY.md).

See [Project status](STATUS.md) for known gaps.
