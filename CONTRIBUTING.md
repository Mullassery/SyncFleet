# Contributing to SyncFleet

Thank you for your interest in contributing!

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/SyncFleet`
3. Create a feature branch: `git checkout -b feature/your-feature`
4. Make your changes
5. Run tests: `cargo test --workspace`
6. Run quality checks: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings`
7. Commit with clear messages
8. Push and open a Pull Request

## Development Setup

```bash
# Install Rust (if not already)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install development tools
rustup component add rustfmt clippy
cargo install cargo-audit cargo-deny cargo-llvm-cov

# Build
cargo build --workspace

# Run tests
cargo test --workspace

# Run with debug logging
RUST_LOG=syncfleet=debug cargo run -- init --node test-node
```

## Code Style

- Follow `rustfmt` (run `cargo fmt`)
- Pass `clippy` with `-D warnings`
- Use `#[must_use]` for fallible operations
- Prefer `anyhow::Result` for application errors
- Use `thiserror` for library error types
- Document public APIs with `///` comments

## Commit Messages

Follow conventional commits:
```
feat: add peer enrollment command
fix: handle connection timeout during sync
docs: update architecture diagram
test: add fault injection for dropped messages
refactor: extract replication protocol to separate module
```

## Pull Request Checklist

- [ ] Tests pass (`cargo test --workspace`)
- [ ] Code formatted (`cargo fmt --check`)
- [ ] No clippy warnings (`cargo clippy --all-targets --all-features -- -D warnings`)
- [ ] No new audit findings (`cargo audit`)
- [ ] Documentation updated if needed
- [ ] CHANGELOG.md updated for user-facing changes
- [ ] Security implications considered

## Testing Requirements

- Unit tests for new functions
- Integration tests for new features
- Fault injection tests for protocol changes
- Property tests for data structures (proptest)

## Security

- Never commit secrets, keys, or tokens
- Report security issues privately (see SECURITY.md)
- Dependencies audited in CI

## License

By contributing, you agree that your contributions will be licensed under the MIT License.