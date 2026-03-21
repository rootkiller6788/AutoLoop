<script setup lang="ts">
import { computed } from "vue";
import SummaryCard from "./SummaryCard.vue";
import SessionBadge from "./SessionBadge.vue";
import type { DashboardSessionSnapshot } from "../../types/snapshot";
import type { AgentRecord } from "../../types/agent";
import type { UiGraphModel } from "../../types/graph";
import { useUiPreferences } from "../../composables/useUiPreferences";

const props = defineProps<{
  snapshot: DashboardSessionSnapshot;
  agents: AgentRecord[];
  graph: UiGraphModel;
  governance: {
    lifecycleEntries: unknown[];
    routeForensics: {
      openCircuits: string[];
      failingTools: string[];
      treatmentShare: number;
    };
  };
}>();
const { t } = useUiPreferences();

const activeCapabilityCount = computed(() =>
  (props.snapshot.capabilityCatalog ?? []).filter((item) => item.status === "active").length
);
const openCircuitCount = computed(
  () => props.governance?.routeForensics?.openCircuits?.length ?? 0
);
const graphNodeCount = computed(() => props.graph?.nodes?.length ?? 0);
const agentCount = computed(() => props.agents?.length ?? 0);

function microsToUsd(value: number): string {
  return `$${(value / 1_000_000).toFixed(2)}`;
}
</script>

<template>
  <section class="summary-strip">
    <SessionBadge :session-id="props.snapshot.sessionId || 'unknown-session'" :anchor="props.snapshot.anchor || 'anchor:unknown'" />
    <p class="summary-copy">{{ props.snapshot.ceoSummary || 'No CEO summary available yet.' }}</p>
    <div class="summary-grid">
      <SummaryCard
        :label="t('summaryVerifier')"
        :value="props.snapshot.verifier?.verdict || 'unknown'"
        :caption="props.snapshot.verifier?.summary || 'No verifier summary'"
        :tone="props.snapshot.readiness ? 'good' : 'warn'"
      />
      <SummaryCard
        :label="t('summaryCapabilities')"
        :value="activeCapabilityCount.toString()"
        caption="Active catalog surfaces"
      />
      <SummaryCard
        :label="t('summaryOpenCircuits')"
        :value="openCircuitCount.toString()"
        :tone="openCircuitCount ? 'warn' : 'good'"
        caption="Runtime breakers"
      />
      <SummaryCard :label="t('summaryGraphNodes')" :value="graphNodeCount.toString()" caption="Visible on canvas" />
      <SummaryCard :label="t('summaryAgents')" :value="agentCount.toString()" caption="Tracked runtime actors" />
      <SummaryCard
        label="Revenue"
        :value="microsToUsd(props.snapshot.business?.revenueMicros ?? 0)"
        caption="Recognized session revenue"
      />
      <SummaryCard
        label="Cost"
        :value="microsToUsd(props.snapshot.business?.costMicros ?? 0)"
        caption="Allocated execution cost"
      />
      <SummaryCard
        label="Profit"
        :value="microsToUsd(props.snapshot.business?.profitMicros ?? 0)"
        :tone="(props.snapshot.business?.profitMicros ?? 0) >= 0 ? 'good' : 'warn'"
        :caption="props.snapshot.business?.riskSummary || 'Risk summary unavailable'"
      />
      <SummaryCard
        label="SLA"
        :value="`${Math.round((props.snapshot.business?.slaSuccessRatio ?? 0) * 100)}%`"
        :tone="(props.snapshot.business?.breachedOrders ?? 0) > 0 ? 'warn' : 'good'"
        :caption="`Breaches: ${props.snapshot.business?.breachedOrders ?? 0}`"
      />
      <SummaryCard
        :label="t('summaryTreatment')"
        :value="`${Math.round(props.snapshot.routeTreatmentShare * 100)}%`"
        caption="Adaptive route share"
      />
    </div>
  </section>
</template>
