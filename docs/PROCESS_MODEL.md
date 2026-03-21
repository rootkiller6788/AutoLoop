# AutoLoop Process Model (Neutral Naming)

This document translates high-level governance metaphors into concrete software layers and control flow.

## Layer Mapping

1. `Policy Control Layer`
Role: global policy, risk thresholds, budget limits, immutable constraints.
Primary modules: `src/security`, `src/runtime`, `src/config`.

2. `Planning Layer`
Role: requirement clarification, objective freeze, decomposition, route proposal.
Primary modules: `src/orchestration`, `src/agent`.

3. `Execution Layer`
Role: capability selection, tool/provider invocation, bounded retries.
Primary modules: `src/tools`, `src/providers`, `src/runtime`.

4. `Resource Kernel Layer`
Role: CPU/memory/time budgets, sandbox boundaries, queue and scheduler control.
Primary modules: `src/runtime`, `src/security`.

5. `Verification Layer`
Role: policy checks, result validation, regression guards, block/allow decisions.
Primary modules: `src/runtime`, `src/security`, `src/observability`.

6. `Learning Layer`
Role: evidence capture, skill consolidation, causal edges, route memory updates.
Primary modules: `src/memory`, `src/rag`, `src/evolution`, `src/hooks`.

7. `Reporting Layer`
Role: operator snapshot, replay timeline, health and risk telemetry.
Primary modules: `src/observability`, `src/dashboard_server.rs`, `dashboard-ui/`.

## Canonical End-to-End Flow

1. `INTENT_INTAKE`
Input is normalized into `RequirementBrief`.

2. `CONSTRAINT_FREEZE`
Acceptance criteria, risk profile, and hard boundaries are frozen.

3. `PLAN_SYNTHESIS`
Planner proposes strategy, critic challenges, judge selects executable plan.

4. `CAPABILITY_ROUTING`
Execution path chooses only `active + verified` capabilities from catalog.

5. `GUARDED_EXECUTION`
Runtime guard enforces budget, timeout, sandbox, and circuit breaker policies.

6. `VERIFICATION_GATE`
Verifier scores output and returns pass / iterate / reject.

7. `LEARNING_COMMIT`
Episodes, witness logs, skill updates, and graph deltas are persisted.

8. `OBSERVABILITY_EMIT`
Snapshot and replay events are emitted to dashboard and audit logs.

## State Machine

- `Pending` -> `Clarifying` -> `Planned` -> `Executing` -> `Verifying` -> `Closed`
- Recovery edges:
  - `Verifying -> Planned` when verifier asks for iteration.
  - `Executing -> Planned` when circuit breaker opens or budget is exceeded.
  - `Any -> Blocked` when policy denies execution.

## Event Dictionary (Recommended)

- `intent.received`
- `constraint.frozen`
- `plan.proposed`
- `plan.selected`
- `route.selected`
- `execution.started`
- `execution.failed`
- `execution.retried`
- `verifier.passed`
- `verifier.rejected`
- `learning.updated`
- `graph.updated`
- `snapshot.emitted`
- `alert.raised`

## Naming Policy (No Political Terms)

Use these neutral terms in new files, APIs, and telemetry keys:

- `chief_agent` -> `strategy_coordinator`
- `ceo_summary` -> `strategy_summary`
- `governance` -> `policy_control`
- `legality_review` -> `policy_validation`
- `audit_committee` -> `verification_pipeline`

For backward compatibility, keep legacy field names only in migration adapters, not in new APIs.
