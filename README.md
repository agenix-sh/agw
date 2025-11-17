# AGW - Agentic Worker

**AGW** is the worker component of the AGX ecosystem - a stateless Rust binary that executes deterministic plan steps.

**For comprehensive architecture documentation, execution layer specifications, and development guidelines, see the [AGEniX central repository](https://github.com/agenix-sh/agenix).**

## Overview

AGW pulls jobs from AGQ (the queue/scheduler), executes Unix and agent tools, and reports results back. It's designed for:

- **Zero external dependencies** - Pure Rust, embedded deployment
- **Deterministic execution** - No LLM calls in workers
- **Security-first** - Session-key authentication, comprehensive input validation
- **Cross-platform** - macOS and Linux support

## Quick Start

### Prerequisites

- Rust 1.83+ (via rustup)
- Install rustup: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

### Build

```bash
cargo build
cargo build --release
```

### Run

```bash
cargo run -- --help

# Example usage
cargo run -- \
  --agq-address 127.0.0.1:6379 \
  --session-key your-session-key \
  --worker-id worker-1
```

### Test

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture
```

## Development

### Before Committing

```bash
# Format code
cargo fmt

# Lint
cargo clippy -- -D warnings

# Test
cargo test

# Security audit
cargo audit
```

### Environment Variables

- `AGQ_ADDRESS` - AGQ server address (default: `127.0.0.1:6379`)
- `AGQ_SESSION_KEY` - Session key for authentication (required)
- `WORKER_ID` - Worker identifier (auto-generated if not provided)
- `HEARTBEAT_INTERVAL` - Heartbeat interval in seconds (default: `30`)
- `CONNECTION_TIMEOUT` - Connection timeout in seconds (default: `10`)

## Architecture

AGW is part of the AGX ecosystem:

1. **AGX** (Planner) - LLM-assisted plan generation
2. **AGQ** (Queue) - Job storage, scheduling, and dispatch
3. **AGW** (Worker) - **This repo** - Executes the actual work

See `docs/ARCHITECTURE.md` for detailed system design.

## Security

AGW implements multiple security layers:

- Session-key authentication for all AGQ communication
- Comprehensive input validation (prevents injection attacks)
- No dynamic code execution
- Principle of least privilege

Security tests are mandatory - see `tests/integration_test.rs`.

## Documentation

- [Architecture Overview](docs/ARCHITECTURE.md)
- [Development Roadmap](docs/ROADMAP.md)
- [Rust Environment Setup](docs/RUST_ENVIRONMENT.md)
- [Claude Development Guide](CLAUDE.md)

## Contributing

See [CLAUDE.md](CLAUDE.md) for detailed contribution guidelines.

## License

MIT
