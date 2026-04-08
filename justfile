default:
    @just --list

build:
    cargo build --release

test:
    cargo test

check:
    cargo clippy -- -D warnings
    cargo fmt -- --check

version:
    ./target/release/review-mcp --version

# Install locally as review-mcp-dev (keeps separate from homebrew production install)
install: build
    cp -f target/release/review-mcp ~/.local/bin/review-mcp-dev.new
    mv -f ~/.local/bin/review-mcp-dev.new ~/.local/bin/review-mcp-dev

# Uninstall local dev binary
uninstall:
    rm -f ~/.local/bin/review-mcp-dev

# Local snapshot release (no publish)
snapshot:
    goreleaser release --snapshot --clean
