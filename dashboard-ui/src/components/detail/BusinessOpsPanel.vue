<script setup lang="ts">
import type { RevenueEventView, WorkOrderView } from "../../types/snapshot";

const props = defineProps<{
  workOrders: WorkOrderView[];
  revenueEvents: RevenueEventView[];
}>();

function microsToUsd(value: number): string {
  return `$${(value / 1_000_000).toFixed(2)}`;
}
</script>

<template>
  <section class="detail-stack">
    <div class="section-title">Business Ops</div>
    <div class="business-grid">
      <article class="business-card">
        <h4>Work Orders</h4>
        <ul>
          <li v-for="order in props.workOrders.slice(0, 6)" :key="order.workOrderId">
            <strong>{{ order.taskId }}</strong>
            <span>{{ order.taskRole }}</span>
            <span>{{ order.serviceTier }}</span>
            <span>{{ order.status }}</span>
          </li>
        </ul>
      </article>
      <article class="business-card">
        <h4>Revenue Events</h4>
        <ul>
          <li v-for="event in props.revenueEvents.slice(0, 6)" :key="event.revenueEventId">
            <strong>{{ event.taskId }}</strong>
            <span>{{ microsToUsd(event.revenueMicros) }}</span>
            <span>{{ microsToUsd(event.costMicros) }}</span>
            <span>{{ microsToUsd(event.profitMicros) }}</span>
          </li>
        </ul>
      </article>
    </div>
  </section>
</template>

