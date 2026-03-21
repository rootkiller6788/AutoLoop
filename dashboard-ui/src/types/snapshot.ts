import type { CapabilityRecord } from "./capability";
import type { SessionReplay } from "./replay";

export interface DashboardGraphLens {
  entities: number;
  relationships: number;
  communities: number;
  forgedCapabilityCount: number;
  topEntities: string[];
}

export interface DashboardVerifierLens {
  verdict: string;
  score: number;
  summary: string;
  failingTools: string[];
}

export interface DashboardBusinessLens {
  revenueMicros: number;
  costMicros: number;
  profitMicros: number;
  marginRatio: number;
  slaSuccessRatio: number;
  breachedOrders: number;
  riskSummary: string;
}

export interface WorkOrderView {
  workOrderId: string;
  taskId: string;
  taskRole: string;
  status: string;
  serviceTier: string;
}

export interface RevenueEventView {
  revenueEventId: string;
  taskId: string;
  revenueMicros: number;
  costMicros: number;
  profitMicros: number;
  source: string;
}

export interface DashboardSessionSnapshot {
  sessionId: string;
  anchor: string;
  ceoSummary: string;
  validationSummary: string;
  routeTreatmentShare: number;
  readiness: boolean;
  capabilityCatalog: CapabilityRecord[];
  proxyForensics: Record<string, unknown>;
  researchHealth: Record<string, unknown>;
  graph: DashboardGraphLens;
  verifier: DashboardVerifierLens;
  business: DashboardBusinessLens;
  workOrders: WorkOrderView[];
  revenueEvents: RevenueEventView[];
  operationsNotes: string[];
  capabilityLifecycle?: Record<string, unknown>;
  runtimeCircuits?: Record<string, unknown>;
  replay?: SessionReplay;
}
