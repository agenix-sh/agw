# Rust Environment Standardization Plan

## AGX Ecosystem Rust Build Environment

**Version:** 1.0
**Status:** Proposed
**Applies to:** AGX, AGQ, AGW

---

## 1. Problem Statement

Currently, there's toolchain conflict between Homebrew's Rust installation (1.79.0) and rustup's installation (1.91.1). This creates:
- Inconsistent builds across the three AGX ecosystem projects
- Dependency resolution failures
- Difficulty in ensuring reproducible builds
- Potential issues for contributors and CI/CD pipelines

---

## 2. Recommended Solution: rustup (Standard Rust Toolchain Manager)

**Recommendation: Use rustup exclusively, remove Homebrew Rust**

### Why rustup?

1. **Official Rust toolchain manager** - Maintained by the Rust project
2. **Per-project toolchains** - Support for `rust-toolchain.toml`
3. **Multi-target support** - Easy cross-compilation for Linux and macOS
4. **Component management** - Integrated clippy, rustfmt, rust-analyzer
5. **Version consistency** - Same toolchain across dev, CI, and production
6. **Industry standard** - Used by most Rust projects

### Why NOT Homebrew for Rust?

- ❌ Lags behind stable releases
- ❌ System-wide installation conflicts with per-project needs
- ❌ No easy toolchain version management
- ❌ Interferes with PATH and cargo's rustc detection
- ❌ Not suitable for CI/CD environments

---

## 3. Implementation Plan

### Phase 1: Environment Setup (One-time per machine)

#### Step 1: Remove Homebrew Rust (if installed)

```bash
# Check what's installed
brew list | grep rust

# Uninstall Homebrew Rust components
brew uninstall rust
brew uninstall rustup  # if installed via brew

# Verify removal
which rustc  # Should not find Homebrew version
```

#### Step 2: Install rustup (if not already installed)

```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Follow prompts - choose default installation
# This installs to ~/.cargo and ~/.rustup

# Add to shell profile (usually auto-added by installer)
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc  # or ~/.bashrc
source ~/.zshrc  # or ~/.bashrc
```

#### Step 3: Install stable toolchain and components

```bash
# Install stable toolchain
rustup install stable
rustup default stable

# Install required components
rustup component add clippy
rustup component add rustfmt
rustup component add rust-src

# Verify installation
rustc --version    # Should show 1.83+ (or latest stable)
cargo --version
clippy-driver --version
rustfmt --version
```

#### Step 4: Add Linux target for cross-compilation (optional but recommended)

```bash
# For macOS developers building for Linux
rustup target add x86_64-unknown-linux-gnu
rustup target add aarch64-unknown-linux-gnu
```

### Phase 2: Project Configuration (All three projects: AGX, AGQ, AGW)

#### Step 1: Create `rust-toolchain.toml` in each project root

```bash
# In agx/, agq/, and agw/ directories
cat > rust-toolchain.toml <<'EOF'
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt", "rust-src"]
profile = "default"
EOF
```

This ensures:
- Everyone uses the same stable version
- Auto-installs required components
- Consistent across all three projects

#### Step 2: Update `Cargo.toml` with minimum Rust version

```toml
[package]
# ... other fields
rust-version = "1.83"  # Minimum required version
edition = "2021"
```

#### Step 3: Add `.cargo/config.toml` (if needed for special build configurations)

Only needed if you have special linker flags or target configurations:

```toml
# .cargo/config.toml
[build]
# Add any project-specific build settings here
# Usually not needed for basic projects
```

#### Step 4: Update `.gitignore`

```gitignore
# Rust
/target/
Cargo.lock  # Unless this is a binary crate (keep for agx, agq, agw)
**/*.rs.bk
*.pdb

# Build artifacts
/build.sh  # Remove if not needed

# IDE
.idea/
.vscode/
*.swp
*.swo
```

**Note:** For binary projects (agx, agq, agw), **commit `Cargo.lock`** to ensure reproducible builds.

### Phase 3: Development Workflow

#### Standard commands (same for all projects):

```bash
# Format code
cargo fmt

# Lint
cargo clippy -- -D warnings

# Build
cargo build
cargo build --release

# Test
cargo test

# Run
cargo run -- [args]
```

#### Security audit:

```bash
# Install cargo-audit (one-time)
cargo install cargo-audit

# Run audit
cargo audit
```

#### Check for unused dependencies:

```bash
# Install cargo-udeps (one-time, requires nightly)
cargo install cargo-udeps --locked

# Run
cargo +nightly udeps
```

---

## 4. CI/CD Configuration

### GitHub Actions Example

Create `.github/workflows/ci.yml` in each project:

```yaml
name: CI

on:
  push:
    branches: [ main ]
  pull_request:
    branches: [ main ]

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    name: Test
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
        rust: [stable]
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ matrix.rust }}
          components: clippy, rustfmt

      - uses: Swatinem/rust-cache@v2

      - name: Format check
        run: cargo fmt -- --check

      - name: Clippy
        run: cargo clippy -- -D warnings

      - name: Test
        run: cargo test

      - name: Build
        run: cargo build --release

  security:
    name: Security Audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - uses: rustsec/audit-check@v2
        with:
          token: ${{ secrets.GITHUB_TOKEN }}
```

---

## 5. Cross-Platform Builds

### macOS → Linux (using cross)

```bash
# Install cross (one-time)
cargo install cross

# Build for Linux on macOS
cross build --target x86_64-unknown-linux-gnu --release
cross build --target aarch64-unknown-linux-gnu --release
```

### Linux → macOS (using zig as linker)

More complex, document if needed.

---

## 6. Target Platforms

### Tier 1 Targets (Officially Supported)

1. **x86_64-unknown-linux-gnu** - Linux x86_64
2. **aarch64-apple-darwin** - macOS ARM (M1/M2/M3)
3. **x86_64-apple-darwin** - macOS Intel

### Optional Tier 2 Targets

- **aarch64-unknown-linux-gnu** - Linux ARM64 (for cloud/edge deployments)

---

## 7. Dependency Management Policy

### General Principles

1. **Minimize dependencies** - Zero external runtime dependencies goal
2. **Pure Rust** - Avoid C bindings where possible
3. **Version pinning** - Use specific versions in Cargo.toml
4. **Regular audits** - Run `cargo audit` before every release
5. **Compatible versions** - Ensure dependencies work with rust-version

### Shared Dependencies Across AGX Ecosystem

Ensure these are version-aligned across all three projects:

```toml
[dependencies]
# Async runtime
tokio = { version = "1.40", features = ["full"] }

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# CLI (agx and agw)
clap = { version = "4.5", features = ["derive", "env"] }

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# RESP protocol (agq and agw)
redis = { version = "0.24", features = ["tokio-comp", "connection-manager"] }
# Note: redis 0.24 is used for compatibility, upgrade when rust-version increases

# Embedded DB (agq only)
redb = "2.1"  # Or latest stable

# UUID (agw)
uuid = { version = "1.10", features = ["v4"] }
```

---

## 8. Documentation for Contributors

### Add to each project's README.md:

```markdown
## Development Setup

### Prerequisites

- Rust 1.83+ (via rustup)
- Install rustup: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

### Quick Start

\`\`\`bash
# Clone and enter project
git clone https://github.com/agenix-sh/agw
cd agw

# Build
cargo build

# Run tests
cargo test

# Run
cargo run -- --help
\`\`\`

### Before Committing

\`\`\`bash
# Format
cargo fmt

# Lint
cargo clippy -- -D warnings

# Test
cargo test

# Security audit
cargo audit
\`\`\`
```

---

## 9. Migration Checklist

### For Each Developer

- [ ] Uninstall Homebrew Rust
- [ ] Install rustup (if not present)
- [ ] Install stable toolchain
- [ ] Add required components (clippy, rustfmt, rust-src)
- [ ] Verify `which rustc` points to `~/.cargo/bin/rustc`
- [ ] Verify `rustc --version` shows 1.83+

### For Each Project (AGX, AGQ, AGW)

- [ ] Add `rust-toolchain.toml`
- [ ] Update `Cargo.toml` with `rust-version = "1.83"`
- [ ] Align shared dependency versions
- [ ] Add/update `.gitignore`
- [ ] Commit `Cargo.lock` (for binary projects)
- [ ] Add CI/CD workflow (`.github/workflows/ci.yml`)
- [ ] Update README.md with development setup
- [ ] Test build on clean machine
- [ ] Document any platform-specific quirks

---

## 10. Testing the Setup

### Verification script:

```bash
#!/bin/bash
# verify-rust-env.sh

echo "🔍 Verifying Rust environment..."
echo

# Check rustc
echo "1. Checking rustc:"
which rustc
rustc --version
echo

# Check cargo
echo "2. Checking cargo:"
which cargo
cargo --version
echo

# Check clippy
echo "3. Checking clippy:"
cargo clippy --version
echo

# Check rustfmt
echo "4. Checking rustfmt:"
cargo fmt --version
echo

# Check for Homebrew Rust (should not exist)
echo "5. Checking for Homebrew Rust (should be empty):"
brew list 2>/dev/null | grep rust || echo "✅ No Homebrew Rust found"
echo

# Check installed toolchains
echo "6. Installed toolchains:"
rustup toolchain list
echo

# Check components
echo "7. Installed components:"
rustup component list --installed
echo

echo "✅ Verification complete!"
```

Save and run:
```bash
chmod +x verify-rust-env.sh
./verify-rust-env.sh
```

---

## 11. Troubleshooting

### Problem: `rustc 1.79.0 is not supported`

**Solution:**
```bash
# Ensure Homebrew Rust is removed
brew uninstall rust

# Ensure PATH is correct
echo $PATH | grep -o '[^:]*cargo[^:]*'  # Should show ~/.cargo/bin first

# Reload shell
exec $SHELL
```

### Problem: `cargo` uses wrong `rustc`

**Solution:**
```bash
# Check what cargo is using
cargo --version -v

# Ensure ~/.cargo/bin is first in PATH
export PATH="$HOME/.cargo/bin:$PATH"

# Make permanent in shell config
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
```

### Problem: Different Rust versions on team

**Solution:**
- Ensure `rust-toolchain.toml` is committed
- rustup will auto-install the correct version when entering project directory

---

## 12. Timeline

- **Week 1**: Environment cleanup and rustup setup
- **Week 2**: Update all three projects with standard configuration
- **Week 3**: CI/CD integration and testing
- **Week 4**: Documentation and team onboarding

---

## 13. Questions?

- rustup docs: https://rust-lang.github.io/rustup/
- Rust toolchain file: https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file

---

**Document maintained by:** AGX Ecosystem Team
**Last updated:** 2025-11-15
