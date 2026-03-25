# AutoLoop

[![Tests](https://img.shields.io/badge/tests-56%20passed-brightgreen)](https://github.com/rootkiller6788/AutoLoop/actions/workflows/ci.yml)
[![Release](https://img.shields.io/badge/release-v0.1.0--alpha-blue)](https://github.com/rootkiller6788/AutoLoop/releases/tag/v0.1.0-alpha)

<img width="1407" height="768" alt="image" src="https://github.com/user-attachments/assets/c95c6f52-f768-44c5-92f6-d6ff50a61ce1" />




## What is AutoLoop?

**AutoLoop is a Rust-native AIOS for governed agent execution.**

It does not just call models and tools.  
It turns ambiguous intent into a **controlled runtime loop**:

**clarify → plan → gate → execute → verify → remember → replay → improve**

That means:

- **vague tasks become structured sessions**
- **all execution goes through policy and runtime guards**
- **results can be verified, audited, and replayed**
- **memory is not passive storage — it feeds future reasoning**
- **learning only upgrades when trust conditions are met**

AutoLoop is for people who want more than "agent demos".  
It is for building **AI systems that can be governed**.

## Why AutoLoop exists

Most agent systems optimize for:

- more tools
- longer chains
- more autonomy
- prettier demos

AutoLoop optimizes for something else:

- **controlled execution**
- **verifiable outcomes**
- **runtime governance**
- **learning with trust boundaries**
- **operator visibility and replay**

In other words:

> **AutoLoop is not another free-form agent wrapper.  
> It is a governed execution runtime for AI systems.**


## 5-Minute Demo

- Windows: [demo/e2e-5min.ps1](demo/e2e-5min.ps1)
- Linux: [demo/e2e-5min.sh](demo/e2e-5min.sh)
- Demo recording checklist: [demo/RECORDING_CHECKLIST.md](demo/RECORDING_CHECKLIST.md)

## Quick Start

### Prerequisites

- Rust toolchain
- Optional: SpacetimeDB CLI
- Optional: Docker / Docker Compose

### Run

```powershell
cargo run --manifest-path .\Cargo.toml -- --message "Build a swarm that uses graph memory and MCP execution" --swarm
```

### Validate

```powershell
cargo check --workspace --manifest-path .\Cargo.toml
cargo test --workspace --manifest-path .\Cargo.toml
```

### Browser Research Runtime

Supported real research backends:

- `browser_fetch`: use a Browserless-style render endpoint
- `playwright_cli`: use local Node + Playwright for browser rendering
- `firecrawl`: use Firecrawl search/scrape APIs

Recommended health checks:

```powershell
cargo run --manifest-path .\Cargo.toml -- system health
cargo run --manifest-path .\Cargo.toml -- crawl status --anchor-id cli:focus
```

## Planning and Learning Governance Flow

```text
┌──────────────────────────────────────────────┐
│              AutoLoop Core Loop              │
└──────────────────────────────────────────────┘

[User Intent]
     |
     v
[Requirement Clarification Agent]
     |
     v
[Policy & Rule Engine] --reject/revise--> [Clarification]
     | approve
     v
[Orchestrator / Planner-Critic-Judge]
     |
     v
[Capability Catalog Selector]
(only active + verified + trusted)
     |
     v
[Runtime Kernel Guard]
(identity/tenant + budget/token + timeout + sandbox + breaker)
     | pass                               | block/fail
     v                                    v
[Execution Pools] ------------------> [Recovery/Degrade/Retry]
     |
     v
[Verifier & Audit Pipeline]
     | pass                              | reject
     v                                   v
[Learning Proposal Builder]          [Back to Plan]
     |
     v
[Learning Gate (Verifier)]
     | promote                           | rollback
     v                                   v
[Memory + GraphRAG Update]        [Keep Previous Skill]
     |
     v
[Routing/Prompt/Capability Strategy Update]
     |
     v
[Observability + Reports + Replay]
     |
     v
[Next Iteration (Repeat)]

```

## Why AutoLoop (3 Core Differentiators)

1. Governed execution, not free-form agent calls: capabilities are cataloged, verified, and routed through runtime guardrails.
2. Memory that participates in decisions: GraphRAG + learning records feed routing, verifier, and capability evolution.
3. End-to-end operability: CLI runtime + SpacetimeDB persistence + dashboard + deployment templates in one repository.

## What Is Implemented in v0.1.0-alpha

- Multi-turn requirement clarification with scope freeze signals
- CEO + planner/critic/judge orchestration artifacts
- Capability catalog and verifier-gated execution path
- GraphRAG snapshot and incremental merge pipeline
- Learning persistence for episodes, skills, causal edges, and witness logs
- Observability records and dashboard snapshot serving

## Current Scope (Honest Boundaries)

- This is an engineering alpha, not a fully production-hardened autonomous platform.
- Real-world provider/tool integrations exist but still need broader compatibility and hardening.
- GraphRAG, verifier policy depth, and learning strategy are functional but still evolving.

## Project Map

- Runtime source: [src](src)
- SpacetimeDB module: [spacetimedb](spacetimedb)
- Adapter crate: [autoloop-spacetimedb-adapter](autoloop-spacetimedb-adapter)
- Dashboard UI: [dashboard-ui](dashboard-ui)
- Deployment assets: [deploy](deploy)
- Tests: [tests](tests)

## Docs

- Deep docs index: [docs/README.md](docs/README.md)
- Process model (neutral naming): [docs/PROCESS_MODEL.md](docs/PROCESS_MODEL.md)
- P1-P13 unified protocol (AI output contract + layer flows): [docs/P1_P13_UNIFIED_PROTOCOL.md](docs/P1_P13_UNIFIED_PROTOCOL.md)
- Contracts RFC v1: [docs/RFC_CONTRACTS_V1.md](docs/RFC_CONTRACTS_V1.md)
- Gray rollout runbook: [docs/ROLLOUT_RUNBOOK.md](docs/ROLLOUT_RUNBOOK.md)
- Architecture: [ARCHITECTURE.md](ARCHITECTURE.md)
- API summary: [API.md](API.md)
- Contributing: [CONTRIBUTING.md](CONTRIBUTING.md)
- Release notes: [RELEASE_NOTES_v0.1.0-alpha.md](RELEASE_NOTES_v0.1.0-alpha.md)
- Public issue backlog: [docs/ISSUE_BACKLOG_v0.1.0-alpha.md](docs/ISSUE_BACKLOG_v0.1.0-alpha.md)

## Notes

- Badges and links are already bound to `rootkiller6788/AutoLoop`.
