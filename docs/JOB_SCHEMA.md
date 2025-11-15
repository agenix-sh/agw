# AGX Job Envelope Schema

This document defines the job payload AGX sends to AGQ/AGW for execution. A job contains the **entire plan** as a single unit, ensuring all steps execute on one worker with local data access.

```json
{
  "job_id": "uuid-1234",
  "plan_id": "uuid-5678",
  "plan_description": "Summarize logs and count errors",
  "steps": [
    {
      "step_number": 1,
      "command": "sort",
      "args": ["-r"],
      "timeout_secs": 30
    },
    {
      "step_number": 2,
      "command": "uniq",
      "args": [],
      "input_from_step": 1,
      "timeout_secs": 30
    }
  ]
}
```

## Field Descriptions

- `job_id` (string): Unique ID for this execution instance. New `job_id` for every submission.
- `plan_id` (string): Stable identifier for the logical plan. Reuse the same `plan_id` when rerunning the same plan over multiple inputs.
- `plan_description` (string, optional): Free-text description of the plan/session intent and reasoning. Useful for search/ops.
- `steps` (array): Ordered, non-empty list of steps to run.
  - `step_number` (u32): 1-based, contiguous.
  - `command` (string): Tool/command identifier (e.g., `sort`, `uniq`, `agx-ocr`).
  - `args` (array of strings): Command arguments.
  - `timeout_secs` (u32, optional): Per-step timeout.
  - `input_from_step` (u32, optional): Use stdout from the referenced prior step as stdin.

## Validation Rules (client-side)
- Steps must be non-empty.
- `step_number` must start at 1 and be contiguous.
- `steps.len()` must not exceed 100 (configurable).
- `input_from_step` must reference a previous step number; self/future refs are invalid.

## Execution Expectations (AGW)
- Execute steps sequentially on a single worker.
- Pipe stdout from `input_from_step` to stdin of the dependent step.
- Halt on first failure; return partial results.

## RESP Submission
- AGX serializes the job envelope to JSON and submits via `PLAN.SUBMIT` (or `JOB.SUBMIT` if AGQ changes the verb).
- AGQ should return a `job_id` on success; AGX stores submission metadata locally alongside the plan buffer.
