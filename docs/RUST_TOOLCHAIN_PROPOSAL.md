# Rust Toolchain Standardization Proposal

**For:** AGX, AGQ, AGW projects
**Date:** 2025-11-15
**Status:** Proposal for review

---

## Summary

Standardize the Rust toolchain across all three AGX ecosystem projects (AGX, AGQ, AGW) by using **rustup exclusively** and removing Homebrew Rust installations.

---

## Current Problem

- **Toolchain conflicts**: Homebrew Rust (1.79.0) vs rustup (1.91.1)
- **Dependency failures**: Modern crate versions require Rust 1.83+
- **Inconsistent builds**: Different toolchains across projects and developers
- **CI/CD issues**: Difficulty ensuring reproducible builds

This is currently blocking AGW development and will likely affect AGX and AGQ.

---

## Proposed Solution

### Use rustup as the official toolchain manager

**What is rustup?**
- Official Rust toolchain installer and version manager
- Maintained by the Rust project
- Industry standard for Rust development

**Why rustup over Homebrew?**
- ✅ Per-project toolchain versions via `rust-toolchain.toml`
- ✅ Always up-to-date with latest Rust releases
- ✅ Integrated component management (clippy, rustfmt)
- ✅ Easy cross-compilation support (macOS ↔ Linux)
- ✅ Consistent with CI/CD environments
- ✅ No PATH conflicts

**Why NOT Homebrew for Rust?**
- ❌ Lags behind stable releases
- ❌ System-wide installation causes conflicts
- ❌ No per-project version control
- ❌ Interferes with cargo's rustc detection

---

## What Changes

### For Developers (One-time setup)

```bash
# 1. Remove Homebrew Rust
brew uninstall rust

# 2. Install rustup (if not present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 3. Install components
rustup component add clippy rustfmt rust-src
cargo install cargo-audit

# 4. Restart terminal
exec $SHELL
```

### For Each Project (AGX, AGQ, AGW)

Add `rust-toolchain.toml` to project root:

```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt", "rust-src"]
profile = "default"
```

Update `Cargo.toml`:

```toml
[package]
# ... existing fields
rust-version = "1.83"  # Minimum supported Rust version
edition = "2021"
```

**That's it!** When you `cd` into the project, rustup auto-installs the correct toolchain.

---

## Benefits

1. **Consistency**: Same toolchain across all three projects
2. **Reliability**: Reproducible builds for all developers
3. **CI/CD**: Easy integration with GitHub Actions
4. **Cross-platform**: Build for Linux from macOS and vice versa
5. **Future-proof**: Easy to update toolchain versions
6. **Zero-friction**: Auto-installs correct version per project

---

## Target Platforms

**Tier 1 (Primary):**
- macOS ARM64 (M1/M2/M3)
- macOS x86_64 (Intel)
- Linux x86_64

**Tier 2 (Future):**
- Linux ARM64 (cloud/edge deployments)

---

## Dependency Alignment

Standardize these versions across all three projects:

```toml
[dependencies]
# Async runtime
tokio = { version = "1.40", features = ["full"] }

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# CLI (agx, agw)
clap = { version = "4.5", features = ["derive", "env"] }

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

---

## Impact Assessment

### Breaking Changes
- **None** for code
- Developers need to switch from Homebrew Rust to rustup (15 min setup)

### Build Changes
- All existing `cargo build`, `cargo test`, etc. commands work identically
- No code changes required
- Builds may be faster with newer compiler

### Timeline
- **Setup**: 15 minutes per developer
- **Project update**: 5 minutes per project (add 2 files)
- **Testing**: Verify builds work on clean machine

---

## Migration Path

### Phase 1: Developer Environment (This week)
Each developer:
1. Uninstall Homebrew Rust
2. Install/verify rustup
3. Install stable toolchain

### Phase 2: Project Configuration (This week)
Each project adds:
1. `rust-toolchain.toml`
2. Update `Cargo.toml` with `rust-version`
3. Verify build works

### Phase 3: CI/CD (Next week)
1. Add GitHub Actions workflows
2. Test on both macOS and Linux runners
3. Add cargo-audit security checks

---

## Questions to Address

**Please review and provide feedback on:**

1. ✅ **Toolchain choice**: Any objections to using rustup as standard?
2. ✅ **Rust version**: Is 1.83 minimum acceptable? (Latest stable is 1.91)
3. ✅ **Dependencies**: Any concerns with the proposed shared dependency versions?
4. ✅ **Timeline**: Is this week too aggressive for developer migration?
5. ✅ **Platforms**: Are we missing any target platforms?
6. ⚠️ **Concerns**: Any project-specific issues this might cause?

---

## Supporting Documentation

Full implementation guide available in: `docs/RUST_ENVIRONMENT.md`

Includes:
- Detailed installation steps
- CI/CD configuration templates
- Cross-compilation guide
- Troubleshooting section
- Verification scripts

---

## Next Steps

1. **Review this proposal** - Each project maintainer reviews
2. **Raise concerns** - Any blockers or issues?
3. **Agree on timeline** - When to migrate?
4. **Execute migration** - Developers update environments
5. **Update projects** - Add toolchain files
6. **Verify builds** - Test on clean machines
7. **Update CI/CD** - Add automated checks

---

## Questions?

- Rustup documentation: https://rust-lang.github.io/rustup/
- Rust toolchain file: https://rust-lang.github.io/rustup/overrides.html

**Contact:** AGW team (currently implementing this for AGW-001)

---

## Approval

Please respond with:
- ✅ **Approved** - Ready to proceed
- ⚠️ **Concerns** - Issues to discuss
- ❌ **Blocked** - Cannot proceed (with reason)

**AGX Project:** ☐ Pending
**AGQ Project:** ☐ Pending
**AGW Project:** ✅ Implementing

---

**Note:** This is a development environment change only. No code changes required. The sooner we standardize, the fewer dependency conflicts we'll face as we build out the ecosystem.
