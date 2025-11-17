# Job Envelope Schema

**This document has been moved to the central Agenix repository.**

## Canonical Location

See: [`agenix/docs/architecture/job-schema.md`](../../agenix/docs/architecture/job-schema.md)

The Job envelope schema defines the structure of Jobs submitted to AGQ and executed by AGW. It is maintained in the central `agenix` repository to ensure consistency across all components.

## Important Update

The canonical schema (v0.2) has been updated to align with the [execution layers nomenclature](../../agenix/docs/architecture/execution-layers.md):

- ~~"steps"~~ → **"tasks"**
- ~~"step_number"~~ → **"task_number"**
- ~~"input_from_step"~~ → **"input_from_task"**

## Quick Reference

A Job contains:
- `job_id` - Unique execution instance identifier
- `plan_id` - Reusable Plan identifier
- `plan_description` - Human-readable intent (optional)
- `tasks` - Ordered array of Tasks to execute

Each Task has:
- `task_number` - 1-based sequential number
- `command` - Tool/AU identifier
- `args` - Command arguments
- `timeout_secs` - Per-task timeout (optional)
- `input_from_task` - Pipe from previous task (optional)

For the complete specification, validation rules, and examples, please refer to the canonical document in the agenix repository.
