<script setup lang="ts">
import { computed } from 'vue'
import { formatBytes } from '../lib/format'
import type { Transfer } from '../lib/store'

const props = defineProps<{ transfer: Transfer; peerName: string }>()
const percent = computed(() => (props.transfer.total ? Math.round((props.transfer.done / props.transfer.total) * 100) : 0))
const label = computed(() => {
  const t = props.transfer
  if (t.status === 'paused') return `Paused, waiting for ${props.peerName}`
  if (t.status === 'failed') return t.reason ?? 'Failed'
  if (t.status === 'done') return t.outgoing ? `Sent to ${props.peerName}` : 'Saved'
  return `${formatBytes(t.done)} of ${formatBytes(t.total)}`
})
</script>

<template>
  <div class="text-[12px]">
    <div class="flex justify-between gap-2">
      <span class="truncate">{{ transfer.name || 'Incoming file' }}</span>
      <span class="shrink-0" :class="transfer.status === 'failed' ? 'text-[var(--danger)]' : 'text-[var(--muted)]'">{{ label }}</span>
    </div>
    <div class="mt-1 h-1 overflow-hidden rounded-full bg-[var(--surface-strong)]">
      <div
        class="h-full rounded-full"
        style="background: linear-gradient(90deg, var(--accent-a), var(--accent-b)); transition: width 200ms ease"
        :style="{ width: `${transfer.status === 'done' ? 100 : percent}%` }"
      />
    </div>
  </div>
</template>
