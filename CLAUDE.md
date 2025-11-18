# Claude Development Guide for AGW

This document provides Claude Code with context and workflow guidelines for contributing to the AGW (Agentic Worker) repository.

---

## Repository Purpose

AGW is the worker component of the AGX ecosystem - a stateless Rust binary that executes deterministic plan steps. It pulls jobs from AGQ (queue), executes Unix and agent tools, and reports results back.

**Key Principles:**
- Pure Rust, zero external dependencies
- Deterministic execution only (no LLM calls in workers)
- Security-first design with session-key authentication
- Comprehensive test coverage
- Unix philosophy: focused, single-purpose

---

## Nomenclature (CRITICAL)

AGW uses precise terminology defined in the canonical execution layers specification.

**Canonical Documentation:**
- **Execution Layers**: `agenix/docs/architecture/execution-layers.md` - authoritative 5-layer model
- **Job Schema**: `agenix/docs/architecture/job-schema.md` - field-level specification for Jobs
- **Schema Files**: `agenix/specs/job.schema.json` - machine-readable JSON schema

**The Five Execution Layers (Summary):**
1. **Task** - Atomic execution unit (single tool/AU call)
2. **Plan** - Ordered list of Tasks (reusable template)
3. **Job** - Runtime instance of a Plan (what AGW executes)
4. **Action** - Many Jobs in parallel (same Plan, different inputs)
5. **Workflow** - Multi-Action orchestration (future)

**AGW's Role:**
- AGW executes **Jobs** (Layer 3) pulled from AGQ
- Each Job contains **Tasks** (Layer 1) to execute sequentially
- AGW never sees Plans directly (those are stored in AGQ)
- AGW reports Job status back to AGQ

**Critical Terminology Rules:**
- ✅ Use "Task" not "step"
- ✅ Use "Job" for runtime execution instances
- ✅ Use "Plan" only when referring to templates stored in AGQ
- ❌ Never use "step", "instruction", "command" (these are ambiguous)

**When implementing code:**
- Variable names: `task_count` not `step_count`
- Function names: `execute_task()` not `execute_step()`
- Struct fields: `job.tasks` not `job.steps`
- Error messages: "Task 3 failed" not "Step 3 failed"

For complete nomenclature details, see `agenix/docs/architecture/execution-layers.md`.

---

## Development Workflow

### 1. Starting Work on an Issue

When assigned an issue (e.g., `AGW-001`):

```bash
# Create a feature branch from main
git checkout main
git pull
git checkout -b agw-001-minimal-rust-worker
```

**Branch naming convention:** `agw-{issue-number}-{short-description}`

### 2. Development Process

**Test-Driven Development:**
1. Write tests first (unit, integration, security)
2. Implement the feature
3. Ensure all tests pass
4. Run security checks

**Security Focus:**
- Validate all inputs rigorously
- Prevent injection attacks (command injection, path traversal)
- Use safe Rust practices (avoid `unsafe` unless absolutely necessary)
- Implement proper error handling (never expose sensitive data in errors)
- Follow OWASP guidelines for relevant attack vectors
- Use `cargo audit` to check for vulnerable dependencies

**Testing Requirements:**
- Unit tests for all public functions
- Integration tests for component interactions
- Security tests for authentication, input validation
- Edge case coverage (empty inputs, malformed data, boundary conditions)
- Error path testing

### 3. Code Quality Checks

Before committing, run:

```bash
# Format code
cargo fmt

# Lint and catch common mistakes
cargo clippy -- -D warnings

# Run all tests
cargo test

# Security audit
cargo audit

# Check for unused dependencies
cargo udeps
```

### 4. Committing Changes

```bash
# Stage changes
git add .

# Commit with descriptive message
git commit -m "AGW-001: Implement minimal Rust worker binary

- Add basic CLI skeleton with clap
- Implement RESP client stub
- Add unit tests for core initialization
- Include security validation for session keys"

# Push to remote
git push -u origin agw-001-minimal-rust-worker
```

**Commit message format:**
```
AGW-{issue}: Brief description (50 chars max)

- Bullet points describing changes
- Focus on what and why, not how
- Reference security considerations
- Note test coverage additions
```

### 5. Creating Pull Requests

```bash
# Create PR using GitHub CLI
gh pr create --title "AGW-001: Implement minimal Rust worker binary" \
  --body "$(cat <<'EOF'
## Summary
Implements the foundational Rust worker binary with:
- Basic CLI structure
- RESP client connection stub
- Session-key authentication placeholder
- Initial test harness

## Security Considerations
- Session key validation implemented
- Input sanitization for worker ID
- Safe handling of network connections

## Test Coverage
- Unit tests: 95%
- Integration tests: Added worker startup/shutdown tests
- Security tests: Auth validation, malformed input handling

## Checklist
- [ ] All tests pass (`cargo test`)
- [ ] No clippy warnings (`cargo clippy -- -D warnings`)
- [ ] Code formatted (`cargo fmt`)
- [ ] Security audit clean (`cargo audit`)
- [ ] Documentation updated
- [ ] Manual testing completed

## Areas for Review
- RESP client error handling approach
- Session key storage strategy
- Worker ID generation method

Closes #1
EOF
)"
```

### 6. AI-Assisted Code Review

**Two-phase review process:**

1. **Claude Review** (security, architecture, test coverage)
   - Ask Claude to review the PR for security vulnerabilities
   - Check test coverage adequacy
   - Validate architectural alignment with AGX ecosystem
   - Review error handling and edge cases

2. **Codex Review** (code quality, performance, Rust idioms)
   - Code style and Rust best practices
   - Performance optimizations
   - Dependency management
   - Documentation clarity

**Example review request:**
```
Please review this PR focusing on:
1. Security: Auth handling, input validation, injection risks
2. Tests: Coverage gaps, edge cases, security test scenarios
3. Architecture: Alignment with AGW design principles
4. Error handling: Proper propagation, no sensitive data leaks
```

### 7. Addressing Review Feedback

```bash
# Make changes based on review
git add .
git commit -m "Address review feedback: improve error handling"
git push

# Continue iteration until both reviewers approve
```

### 8. Merging

Once approved by both AI reviewers and all checks pass:

```bash
# Merge via GitHub (squash commits for clean history)
gh pr merge --squash --delete-branch
```

---

## Testing Guidelines

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_key_validation() {
        // Test valid key
        assert!(validate_session_key("valid-key-format").is_ok());

        // Test invalid keys
        assert!(validate_session_key("").is_err());
        assert!(validate_session_key("../etc/passwd").is_err());
        assert!(validate_session_key("key;rm -rf /").is_err());
    }
}
```

### Integration Tests
Place in `tests/` directory:
```rust
// tests/worker_lifecycle.rs
#[test]
fn test_worker_startup_and_shutdown() {
    // Test full worker lifecycle
}
```

### Security Tests
```rust
#[test]
fn test_command_injection_prevention() {
    // Validate input sanitization prevents command injection
}

#[test]
fn test_path_traversal_prevention() {
    // Ensure file paths are validated
}
```

---

## Security Checklist

For every PR, verify:

- [ ] Input validation on all external data
- [ ] No use of `unsafe` without justification and safety comments
- [ ] Error messages don't leak sensitive information
- [ ] Authentication checks before privileged operations
- [ ] No hardcoded credentials or secrets
- [ ] Dependencies audited (`cargo audit`)
- [ ] Command execution sanitized (prevent injection)
- [ ] File paths validated (prevent traversal)
- [ ] Network data validated before processing
- [ ] Resource limits enforced (prevent DoS)

---

## Common Patterns

### Error Handling
```rust
use anyhow::{Context, Result};

fn connect_to_queue(addr: &str) -> Result<Connection> {
    validate_address(addr)
        .context("Invalid queue address")?;

    Connection::new(addr)
        .context("Failed to connect to queue")
}
```

### Input Validation
```rust
fn validate_worker_id(id: &str) -> Result<()> {
    if id.is_empty() {
        bail!("Worker ID cannot be empty");
    }

    if !id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        bail!("Worker ID contains invalid characters");
    }

    Ok(())
}
```

---

## Shared Claude Code Skills & Agents

This repository uses shared Claude Code configuration from the agenix repo (via git submodule at `agenix-shared/.claude/`):

### Available Skills (Auto-Activated)
- **agenix-architecture** - Enforces execution layer nomenclature (Task/Plan/Job/Action/Workflow)
- **agenix-security** - OWASP Top 10, zero-trust principles, constant-time comparisons
- **agenix-testing** - TDD practices, 80% coverage minimum, 100% for security-critical code
- **rust-agenix-standards** - Rust error handling, async patterns, type safety idioms

### Available Agents (Explicit Invocation)
- **rust-engineer** - Deep Rust expertise for async, performance, safety
- **security-auditor** - Vulnerability detection and prevention
- **github-manager** - Issue/PR creation with proper templates and labels
- **multi-repo-coordinator** - Cross-repository change coordination

See `.claude/README.md` for detailed documentation on skill activation and agent usage.

---

## Architecture Alignment

When implementing features, ensure:

1. **No LLM calls in workers** - Workers execute deterministically
2. **RESP protocol compliance** - All AGQ communication via RESP
3. **Session-key auth** - Every command requires valid session key
4. **Stateless execution** - Workers don't maintain plan state
5. **Tool abstraction** - Unix and agent tools treated uniformly
6. **Graceful degradation** - Handle missing tools/dependencies properly

---

## Questions?

- Architecture questions: Review `docs/ARCHITECTURE.md`
- Roadmap context: Check `docs/ROADMAP.md`
- Issue tracking: See GitHub issues tagged `AGW-XXX`

---

**Remember:** Security and testing are not optional. Every PR must demonstrate both.
