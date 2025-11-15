# Architecture Memo: Job Structure for Distributed Execution

**Date:** 2025-11-15
**From:** AGW Team
**To:** AGQ and AGX Teams
**Subject:** Job Structure Change - Entire Plan Per Job Required

## Problem Statement

The current job structure sends individual steps one at a time to the queue. In a distributed environment with multiple AGW workers, this creates a critical architectural flaw:

```
AGQ Queue: [Step 1] [Step 2] [Step 3]
           ↓        ↓        ↓
Workers:   AGW-A    AGW-B    AGW-C
```

**Issue:** Step 2 depends on Step 1's output, but executes on a different worker (AGW-B) that doesn't have access to AGW-A's execution results.

## Required Change

**Jobs must contain the entire execution plan, not individual steps.**

This ensures atomic execution of all steps on a single worker, with step outputs available to subsequent steps in the same plan.

## New Job Structure

### Current Structure (DEPRECATED)
```json
{
  "id": "job-123",
  "plan_id": "plan-456",
  "step_number": 1,
  "tool": "unix",
  "command": "ls",
  "args": ["-la"],
  "timeout": 30
}
```

### New Structure (REQUIRED)
```json
{
  "id": "job-123",
  "plan_id": "plan-456",
  "steps": [
    {
      "step_number": 1,
      "tool": "unix",
      "command": "ls",
      "args": ["-la", "/tmp"],
      "timeout": 30
    },
    {
      "step_number": 2,
      "tool": "unix",
      "command": "grep",
      "args": ["pattern"],
      "input_from_step": 1,
      "timeout": 30
    },
    {
      "step_number": 3,
      "tool": "agx-ocr",
      "command": "process-image",
      "args": ["--format", "json"],
      "input_from_step": 2,
      "timeout": 120
    }
  ]
}
```

## Step Input/Output Handling

### Input Sources

Each step can specify where its input comes from:

- **`input_from_step: <number>`** - Use stdout from specified step
- **No input field** - No stdin (default behavior)

### Output Capture

Each step produces:
- `stdout` - Standard output (becomes input for dependent steps)
- `stderr` - Standard error (logged/reported separately)
- `exit_code` - Process exit code
- `success` - Boolean (true if exit_code == 0)

### Error Handling

If any step fails (`exit_code != 0`):
1. AGW halts execution of remaining steps
2. AGW posts partial results to AGQ
3. AGQ marks job as failed
4. AGX receives failure notification with error details

## Impact on Components

### AGX (Planner)
**Required Changes:**
- Generate entire execution plan upfront
- Create single job JSON containing all steps
- Define step dependencies via `input_from_step` field
- Post complete plan to AGQ as one job

**Example Planning:**
```python
plan = {
    "id": generate_job_id(),
    "plan_id": self.plan_id,
    "steps": [
        {"step_number": 1, "tool": "unix", "command": "cat", "args": ["file.txt"]},
        {"step_number": 2, "tool": "unix", "command": "wc", "args": ["-l"], "input_from_step": 1}
    ]
}
agq.push_job(plan)
```

### AGQ (Queue)
**Required Changes:**
- Accept job JSON with `steps` array instead of single step fields
- Validate job structure before queuing
- No changes to RESP protocol (still LPUSH/BRPOP)
- Job storage remains JSON strings

**Validation Rules:**
- Job must have `id`, `plan_id`, and `steps` fields
- `steps` must be non-empty array
- Each step must have `step_number`, `tool`, `command`
- `input_from_step` must reference valid previous step number
- No circular dependencies allowed

### AGW (Worker)
**Required Changes:**
- Parse job with `steps` array
- Execute steps sequentially in order
- Capture each step's stdout/stderr
- Pass stdout from step N to stdin of step N+1 (if `input_from_step` specified)
- Halt execution on first failure
- Post comprehensive result containing all step outputs

**Execution Flow:**
```
BRPOP queue:ready
  ↓
Parse Job with steps[]
  ↓
For each step in order:
  ├─ Execute command
  ├─ Capture stdout/stderr/exit_code
  ├─ If input_from_step: pipe previous stdout to stdin
  ├─ If exit_code != 0: halt and report failure
  └─ Store outputs for next step
  ↓
Post complete results to AGQ
```

## Result Structure

AGW posts results back to AGQ with outputs from all executed steps:

```json
{
  "job_id": "job-123",
  "plan_id": "plan-456",
  "success": true,
  "step_results": [
    {
      "step_number": 1,
      "stdout": "file1.txt\nfile2.txt\n",
      "stderr": "",
      "exit_code": 0,
      "success": true
    },
    {
      "step_number": 2,
      "stdout": "2\n",
      "stderr": "",
      "exit_code": 0,
      "success": true
    }
  ]
}
```

## Benefits

✅ **Atomic Execution** - Entire plan runs on single worker
✅ **Data Locality** - All step outputs available locally
✅ **No Distributed State** - No need to share outputs between workers
✅ **Clear Ownership** - One worker owns entire job lifecycle
✅ **Simpler Error Handling** - Worker handles all step failures
✅ **Easier Debugging** - Complete execution trace in one place

## Migration Path

1. **Phase 1:** AGX and AGQ implement new job structure
2. **Phase 2:** AGW implements multi-step execution (current PR #15 will be updated)
3. **Phase 3:** Deprecate old single-step format
4. **Phase 4:** Remove backward compatibility

## Questions for Discussion

1. **Max steps per job?** Suggest limit of 100 steps for safety
2. **Max total timeout?** Sum of all step timeouts, or separate job timeout?
3. **Partial result posting?** Should AGW post results for successful steps even if later step fails?
4. **Step dependencies?** Currently simple linear `input_from_step`, need more complex DAG support?
5. **Parallel step execution?** Future enhancement for independent steps?

## Timeline

- **Week 1:** Finalize specification (this memo)
- **Week 2:** AGX implements new job generation
- **Week 2:** AGQ implements new job validation
- **Week 2:** AGW implements multi-step execution (update PR #15)
- **Week 3:** Integration testing
- **Week 4:** Production deployment

## Contact

For questions or concerns about this change, please coordinate via the architecture channel.

---

**Action Required:**
- [ ] AGX: Review and confirm job generation changes
- [ ] AGQ: Review and confirm job validation changes
- [ ] AGW: Update PR #15 to implement multi-step execution
- [ ] All: Provide feedback on open questions by EOW

---

*This memo supersedes previous assumptions about single-step job execution.*
