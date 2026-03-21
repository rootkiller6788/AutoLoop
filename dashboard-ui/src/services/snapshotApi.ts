import type { DashboardSessionSnapshot } from "../types/snapshot";
import type { CapabilityRecord } from "../types/capability";

export const DEFAULT_SNAPSHOT_BASE = "http://127.0.0.1:8787";

export function buildDashboardUrl(sessionId: string, baseUrl = DEFAULT_SNAPSHOT_BASE): string {
  return `${baseUrl.replace(/\/$/, "")}/api/dashboard/${encodeURIComponent(sessionId)}`;
}

export function buildCatalogUrl(sessionId: string, baseUrl = DEFAULT_SNAPSHOT_BASE): string {
  return `${baseUrl.replace(/\/$/, "")}/api/catalog/${encodeURIComponent(sessionId)}`;
}

export async function fetchDashboardSnapshot(url: string): Promise<DashboardSessionSnapshot> {
  const response = await fetch(url);
  const raw = await response.text();
  return normalizeDashboardSnapshot(JSON.parse(raw) as Record<string, unknown>);
}

function normalizeDashboardSnapshot(raw: Record<string, unknown>): DashboardSessionSnapshot {
  const capabilityCatalog = Array.isArray(raw.capabilityCatalog)
    ? raw.capabilityCatalog
    : Array.isArray(raw.capability_catalog)
      ? raw.capability_catalog
      : [];
  const graphRaw = asRecord(raw.graph);
  const verifierRaw = asRecord(raw.verifier);
  const businessRaw = asRecord(raw.business);

  return {
    sessionId: asString(raw.sessionId ?? raw.session_id),
    anchor: asString(raw.anchor),
    ceoSummary: asString(raw.ceoSummary ?? raw.ceo_summary),
    validationSummary: asString(raw.validationSummary ?? raw.validation_summary),
    routeTreatmentShare: asNumber(raw.routeTreatmentShare ?? raw.route_treatment_share),
    readiness: Boolean(raw.readiness),
    capabilityCatalog: capabilityCatalog.map(normalizeCapability),
    proxyForensics: asRecord(raw.proxyForensics ?? raw.proxy_forensics),
    researchHealth: asRecord(raw.researchHealth ?? raw.research_health),
    graph: {
      entities: asNumber(graphRaw.entities),
      relationships: asNumber(graphRaw.relationships),
      communities: asNumber(graphRaw.communities),
      forgedCapabilityCount: asNumber(
        graphRaw.forgedCapabilityCount ?? graphRaw.forged_capability_count
      ),
      topEntities: asStringArray(graphRaw.topEntities ?? graphRaw.top_entities)
    },
    verifier: {
      verdict: asString(verifierRaw.verdict),
      score: asNumber(verifierRaw.score),
      summary: asString(verifierRaw.summary),
      failingTools: asStringArray(verifierRaw.failingTools ?? verifierRaw.failing_tools)
    },
    business: {
      revenueMicros: asNumber(businessRaw.revenueMicros ?? businessRaw.revenue_micros),
      costMicros: asNumber(businessRaw.costMicros ?? businessRaw.cost_micros),
      profitMicros: asNumber(businessRaw.profitMicros ?? businessRaw.profit_micros),
      marginRatio: asNumber(businessRaw.marginRatio ?? businessRaw.margin_ratio),
      slaSuccessRatio: asNumber(businessRaw.slaSuccessRatio ?? businessRaw.sla_success_ratio),
      breachedOrders: asNumber(businessRaw.breachedOrders ?? businessRaw.breached_orders),
      riskSummary: asString(businessRaw.riskSummary ?? businessRaw.risk_summary)
    },
    workOrders: normalizeWorkOrders(raw.workOrders ?? raw.work_orders),
    revenueEvents: normalizeRevenueEvents(raw.revenueEvents ?? raw.revenue_events),
    operationsNotes: asStringArray(raw.operationsNotes ?? raw.operations_notes),
    capabilityLifecycle: asRecord(raw.capabilityLifecycle ?? raw.capability_lifecycle),
    runtimeCircuits: asRecord(raw.runtimeCircuits ?? raw.runtime_circuits)
  };
}

function normalizeCapability(raw: unknown): CapabilityRecord {
  const record = asRecord(raw);
  return {
    name: asString(record.name ?? record.tool_name),
    status: asString(record.status),
    approval: asString(record.approval ?? record.approval_status),
    health: asNumber(record.health ?? record.health_score),
    scope: asString(record.scope),
    risk: asString(record.risk)
  };
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function asNumber(value: unknown): number {
  return typeof value === "number" ? value : 0;
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.map((item) => String(item)) : [];
}

function normalizeWorkOrders(value: unknown) {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.map((item) => {
    const record = asRecord(item);
    return {
      workOrderId: asString(record.workOrderId ?? record.work_order_id),
      taskId: asString(record.taskId ?? record.task_id),
      taskRole: asString(record.taskRole ?? record.task_role),
      status: asString(record.status),
      serviceTier: asString(record.serviceTier ?? record.service_tier)
    };
  });
}

function normalizeRevenueEvents(value: unknown) {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.map((item) => {
    const record = asRecord(item);
    return {
      revenueEventId: asString(record.revenueEventId ?? record.revenue_event_id),
      taskId: asString(record.taskId ?? record.task_id),
      revenueMicros: asNumber(record.revenueMicros ?? record.revenue_micros),
      costMicros: asNumber(record.costMicros ?? record.cost_micros),
      profitMicros: asNumber(record.profitMicros ?? record.profit_micros),
      source: asString(record.source)
    };
  });
}
