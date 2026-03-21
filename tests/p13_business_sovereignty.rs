use autoloop::{
    AutoLoopApp,
    config::AppConfig,
    orchestration::{ExecutionReport, RequirementBrief, RoutingContext, SwarmDeliberation, SwarmOutcome, SwarmTask, ValidationReport},
    providers::OptimizationProposal,
    runtime::{CapabilityRegressionSuite, VerifierReport, VerifierVerdict},
    tools::CapabilityLifecycleReport,
};

fn report(task_id: &str, role: &str, outcome_score: i32, guard: &str) -> ExecutionReport {
    ExecutionReport {
        task: SwarmTask {
            task_id: task_id.into(),
            agent_name: format!("{role}-agent"),
            role: role.into(),
            objective: format!("{role} objective"),
            depends_on: vec![],
        },
        output: "ok".into(),
        tool_used: Some("mcp::local-mcp::invoke".into()),
        mcp_server: Some("local-mcp".into()),
        invocation_payload: Some("{}".into()),
        outcome_score,
        route_variant: "control".into(),
        control_score: outcome_score,
        treatment_score: outcome_score,
        guard_decision: guard.into(),
    }
}

fn outcome(session_id: &str, execution_reports: Vec<ExecutionReport>) -> SwarmOutcome {
    SwarmOutcome {
        brief: RequirementBrief {
            anchor_id: format!("anchor:{session_id}"),
            original_request: "build".into(),
            clarified_goal: "build".into(),
            frozen_scope: "scope".into(),
            open_questions: vec![],
            acceptance_criteria: vec![],
            clarification_turns: vec![],
            confirmation_required: false,
        },
        optimization_proposal: OptimizationProposal {
            title: "title".into(),
            change_target: "target".into(),
            hypothesis: "hypothesis".into(),
            expected_gain: "gain".into(),
            risk: "risk".into(),
            patch_outline: vec![],
            evaluation_focus: "focus".into(),
        },
        routing_context: RoutingContext {
            history_records: vec![],
            execution_metrics: vec![],
            graph_signals: Default::default(),
            pending_event_count: 0,
            learning_evidence: vec![],
            skill_success_rate: 0.0,
            causal_confidence: 0.0,
            forged_tool_coverage: 0,
            session_ab_stats: None,
            task_ab_stats: Default::default(),
            tool_ab_stats: Default::default(),
            server_ab_stats: Default::default(),
            agent_reputations: Default::default(),
            route_biases: vec![],
        },
        ceo_summary: "summary".into(),
        deliberation: SwarmDeliberation {
            planner_notes: "p".into(),
            critic_notes: "c".into(),
            planner_rebuttal: "r".into(),
            judge_notes: "j".into(),
            arbitration_summary: "a".into(),
            round_count: 1,
            rounds: vec![],
            final_execution_order: vec![],
            consensus_signals: vec![],
        },
        tasks: vec![],
        execution_reports,
        verifier_report: VerifierReport {
            verifier_name: "verifier".into(),
            verdict: VerifierVerdict::Pass,
            overall_score: 0.9,
            summary: "ok".into(),
            task_judgements: vec![],
            route_reports: vec![],
            capability_regression: CapabilityRegressionSuite {
                suite_name: "suite".into(),
                all_passed: true,
                score: 1.0,
                failing_tools: vec![],
                cases: vec![],
                summary: "ok".into(),
            },
            recommended_actions: vec![],
        },
        validation: ValidationReport {
            ready: true,
            summary: "ok".into(),
            follow_up_tasks: vec![],
            verifier_summary: "ok".into(),
        },
        knowledge_update: autoloop::rag::GraphKnowledgeUpdate {
            document_id: 1,
            local_context_summary: "local".into(),
            global_context_summary: "global".into(),
            task_capability_map_summary: "map".into(),
            snapshot_json: "{}".into(),
        },
    }
}

#[tokio::test]
async fn p13_reports_include_income_cost_profit_and_risk() {
    let app = AutoLoopApp::new(AppConfig::default());
    let session_id = "p13-business-report";
    let sample_outcome = outcome(
        session_id,
        vec![
            report("task-1", "Execution", 3, "Allow"),
            report("task-2", "Security", 2, "Allow"),
        ],
    );

    app.observability
        .persist_swarm_observability(
            &app.spacetimedb,
            session_id,
            &sample_outcome,
            &CapabilityLifecycleReport {
                total_lineages: 0,
                active_capabilities: 0,
                deprecated_capabilities: 0,
                rollback_ready_capabilities: 0,
                entries: vec![],
            },
            0,
        )
        .await
        .expect("persist");

    let margin = serde_json::from_str::<serde_json::Value>(
        &app.export_knowledge(session_id, "margin").await.expect("margin"),
    )
    .expect("margin json");
    let sla = serde_json::from_str::<serde_json::Value>(
        &app.export_knowledge(session_id, "sla").await.expect("sla"),
    )
    .expect("sla json");
    let business = serde_json::from_str::<serde_json::Value>(
        &app.export_knowledge(session_id, "business")
            .await
            .expect("business"),
    )
    .expect("business json");

    assert!(margin.get("recognized_revenue_micros").and_then(serde_json::Value::as_u64).unwrap_or(0) > 0);
    assert!(margin.get("allocated_cost_micros").is_some());
    assert!(business.get("risk_summary").and_then(serde_json::Value::as_str).is_some());
    assert!(sla.get("sla_success_ratio").and_then(serde_json::Value::as_f64).unwrap_or(0.0) >= 0.0);
}

#[tokio::test]
async fn p13_order_and_revenue_are_traceable_to_task_ids() {
    let app = AutoLoopApp::new(AppConfig::default());
    let session_id = "p13-order-trace";
    let sample_outcome = outcome(session_id, vec![report("task-trace", "Execution", 2, "Allow")]);

    app.observability
        .persist_swarm_observability(
            &app.spacetimedb,
            session_id,
            &sample_outcome,
            &CapabilityLifecycleReport {
                total_lineages: 0,
                active_capabilities: 0,
                deprecated_capabilities: 0,
                rollback_ready_capabilities: 0,
                entries: vec![],
            },
            0,
        )
        .await
        .expect("persist");

    let work_orders = serde_json::from_str::<Vec<serde_json::Value>>(
        &app.export_knowledge(session_id, "work-orders")
            .await
            .expect("work-orders"),
    )
    .expect("work-orders json");
    let revenue_events = serde_json::from_str::<Vec<serde_json::Value>>(
        &app.export_knowledge(session_id, "revenue")
            .await
            .expect("revenue"),
    )
    .expect("revenue json");

    assert!(!work_orders.is_empty());
    assert!(!revenue_events.is_empty());
    assert!(work_orders.iter().any(|item| item.get("task_id").and_then(serde_json::Value::as_str) == Some("task-trace")));
    assert!(revenue_events.iter().any(|item| item.get("task_id").and_then(serde_json::Value::as_str) == Some("task-trace")));
}

#[tokio::test]
async fn p13_sla_breach_flow_is_visible_in_reports() {
    let app = AutoLoopApp::new(AppConfig::default());
    let session_id = "p13-sla-breach";
    let sample_outcome = outcome(
        session_id,
        vec![
            report("task-breach", "Security", -2, "Blocked"),
            report("task-ok", "Execution", 2, "Allow"),
        ],
    );

    app.observability
        .persist_swarm_observability(
            &app.spacetimedb,
            session_id,
            &sample_outcome,
            &CapabilityLifecycleReport {
                total_lineages: 0,
                active_capabilities: 0,
                deprecated_capabilities: 0,
                rollback_ready_capabilities: 0,
                entries: vec![],
            },
            0,
        )
        .await
        .expect("persist");

    let sla = serde_json::from_str::<serde_json::Value>(
        &app.export_knowledge(session_id, "sla").await.expect("sla"),
    )
    .expect("sla json");
    let breaches = sla
        .get("breach_tasks")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(breaches.iter().any(|value| value.as_str() == Some("task-breach")));
}
