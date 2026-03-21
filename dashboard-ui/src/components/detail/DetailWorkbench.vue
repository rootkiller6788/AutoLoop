<script setup lang="ts">
import { ref } from "vue";
import PanelFrame from "../common/PanelFrame.vue";
import type { DashboardSessionSnapshot } from "../../types/snapshot";
import type { SessionReplay } from "../../types/replay";
import type { UiGraphNode } from "../../types/graph";
import type { CapabilityLifecycleEntry, CapabilityRecord } from "../../types/capability";
import DetailTabs from "./DetailTabs.vue";
import NodeDetailTab from "./NodeDetailTab.vue";
import EvolutionTimelineTab from "./EvolutionTimelineTab.vue";
import GovernanceParamsTab from "./GovernanceParamsTab.vue";
import BusinessOpsPanel from "./BusinessOpsPanel.vue";

defineProps<{
  snapshot: DashboardSessionSnapshot;
  replay: SessionReplay | null;
  selectedNode: UiGraphNode | null;
  relatedNodes: UiGraphNode[];
  capabilities: CapabilityRecord[];
  capabilityLifecycle: CapabilityLifecycleEntry[];
  routeForensics: {
    openCircuits: string[];
    failingTools: string[];
    treatmentShare: number;
  };
  pendingTool?: string;
  selectedReplayTool?: string;
  selectedReplayIndex?: number;
}>();

const activeTab = ref<"detail" | "evolution" | "governance">("detail");

const emit = defineEmits<{
  govern: [action: "verify" | "deprecate" | "rollback", tool: string];
  selectReplayEvent: [payload: { index: number; tool?: string }];
}>();
</script>

<template>
  <aside class="detail-workbench">
    <DetailTabs v-model:active-tab="activeTab" />

    <PanelFrame v-if="activeTab === 'detail'" title="Node Detail" subtitle="Graph and runtime source mapping">
      <NodeDetailTab :selected-node="selectedNode" :related-nodes="relatedNodes" />
    </PanelFrame>

    <PanelFrame v-else-if="activeTab === 'evolution'" title="Evolution Timeline" subtitle="Reasoning | Execution | Evolution">
      <EvolutionTimelineTab
        :replay="replay"
        :route-forensics="routeForensics"
        :selected-replay-tool="selectedReplayTool"
        :selected-replay-index="selectedReplayIndex"
        @select-event="emit('selectReplayEvent', $event)"
      />
    </PanelFrame>

    <PanelFrame v-else title="Governance & Params" subtitle="Lifecycle posture and bounded tuning">
      <GovernanceParamsTab
        :capabilities="capabilities"
        :capability-lifecycle="capabilityLifecycle"
        :pending-tool="pendingTool"
        @govern="emit('govern', $event[0], $event[1])"
      />
      <BusinessOpsPanel
        :work-orders="snapshot.workOrders ?? []"
        :revenue-events="snapshot.revenueEvents ?? []"
      />
    </PanelFrame>
  </aside>
</template>
