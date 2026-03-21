# AutoLoop Gray Rollout Runbook

## 1) Shadow（只记录不拦截）

```powershell
cd D:\AutoLoop\autoloop-app
$env:OPENAI_API_KEY="你的key"
cargo run -- --config deploy/config/autoloop.dev.toml system status
cargo run -- --config deploy/config/autoloop.dev.toml system health
```

`autoloop.dev.toml` 关键值：

- `gate_mode = "shadow"`
- `gate_enforce_ratio = 0.2`（shadow 下仅观测，不实际拦截）
- `rollback_contract_version = "v1"`

## 2) Canary 10%（小流量强制门禁）

先改 `prod` 配置：

- `gate_mode = "canary"`
- `gate_enforce_ratio = 0.1`

然后执行：

```powershell
cargo run -- --config deploy/config/autoloop.prod.toml system status
cargo run -- --config deploy/config/autoloop.prod.toml system health
cargo run -- --config deploy/config/autoloop.prod.toml --session canary-10 --swarm --message "run canary workload"
```

可选人工审批命令（Operator Control Plane）：

```powershell
cargo run -- --config deploy/config/autoloop.prod.toml --session canary-10 system approve --reason "canary 10% approved"
cargo run -- --config deploy/config/autoloop.prod.toml --session canary-10 system reject --reason "canary 10% blocked by operator"
```

## 3) Canary 30%

把 `gate_enforce_ratio` 改为 `0.3`，然后：

```powershell
cargo run -- --config deploy/config/autoloop.prod.toml system status
cargo run -- --config deploy/config/autoloop.prod.toml --session canary-30 --swarm --message "run canary workload"
```

## 4) Full（全量强制门禁）

把配置改为：

- `gate_mode = "full"`
- `gate_enforce_ratio = 1.0`

执行：

```powershell
cargo run -- --config deploy/config/autoloop.prod.toml system status
cargo run -- --config deploy/config/autoloop.prod.toml --session full-rollout --swarm --message "run full workload"
```

## 5) Rollback（回退到上一个稳定接口版本）

回滚策略：

- 保持 `gate_mode = "full"`（或临时切 `shadow` 降风险）
- 把 `rollback_contract_version` 设为上一个稳定版本（例如 `v1`）

执行：

```powershell
cargo run -- --config deploy/config/autoloop.prod.toml system status
cargo run -- --config deploy/config/autoloop.prod.toml --session rollback-check system health
```

## 对应 autocog system 操作序列建议（每阶段固定）

1. `system status`
2. `system health`
3. 业务流量命令（`--swarm --message ...`）
4. 必要时 `system approve/reject --reason ...`
5. 观察后再升比例或回滚
