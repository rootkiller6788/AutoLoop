# RFC: Contracts v1 Freeze

Status: Accepted  
Version: `v1`  
Owner: AutoLoop Core

## Scope

This RFC freezes the P0 interface contract in `src/contracts/` and defines compatibility policy for all future changes.

## Frozen Interfaces

The following traits are locked for `v1`:

1. `OperatorControlPlane`
2. `PolicyRuleEngine`
3. `OrchestratorScheduler`
4. `ExecutionPool`
5. `RuntimeKernel`
6. `VerifierAuditPipeline`
7. `LearningGraphEngine`
8. `ReportingObservability`

## Frozen Core DTO

The following DTO are locked for `v1`:

1. `Intent`
2. `PolicyDecision`
3. `ExecutionPlan`
4. `TaskEnvelope`
5. `RunReceipt`
6. `VerificationVerdict`
7. `LearningDelta`
8. `ReportArtifact`

## Versioning Policy

1. Additive fields in DTO are allowed only when marked optional and backward compatible.
2. Trait method signature changes are forbidden in-place.
3. Breaking changes require a new version namespace (`v2`) and adapter bridge.
4. `CONTRACT_VERSION` in `src/contracts/version.rs` is the canonical runtime contract marker.

## Migration Rule

1. Existing `v1` callers must keep working until explicitly deprecated.
2. Deprecation requires one release cycle overlap.
3. Removal is allowed only after compatibility adapters are available.

## Runtime Gate Rule

Starting in P2, tool/MCP execution paths must go through `RuntimeKernel::execute(TaskEnvelope)` to enforce:

1. Budget constraints (CPU, memory, timeout, token)
2. I/O policy (allow/deny paths)
3. Guard decisions and circuit breaker states
4. Auditable execution evidence
