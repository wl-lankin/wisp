<script setup lang="ts">
import { computed } from 'vue'
import DeviceOrb from './DeviceOrb.vue'
import { layoutOrbs } from '../lib/constellation'
import type { Os, PeerStatus } from '../lib/types'

const props = withDefaults(
  defineProps<{
    selfName: string
    selfOs: Os
    peers: PeerStatus[]
    pendingByPeer: Record<string, number>
    dropTarget: string | null
    width?: number
    height?: number
  }>(),
  { width: 328, height: 220 },
)
defineEmits<{ select: [peerId: string] }>()

const placed = computed(() => {
  const spots = layoutOrbs(props.peers.map((p) => p.peer.deviceId), props.width, props.height)
  return props.peers.map((p, i) => ({ status: p, ...spots[i] }))
})
</script>

<template>
  <div class="relative" :style="{ width: `${width}px`, height: `${height}px` }">
    <svg class="pointer-events-none absolute inset-0" :width="width" :height="height">
      <defs>
        <linearGradient id="wisp-link" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stop-color="var(--accent-a)" />
          <stop offset="1" stop-color="var(--accent-b)" />
        </linearGradient>
      </defs>
      <line
        v-for="p in placed"
        :key="p.id"
        :x1="width / 2"
        :y1="height / 2"
        :x2="p.x"
        :y2="p.y"
        :stroke="p.status.online ? 'url(#wisp-link)' : 'var(--border)'"
        :stroke-width="p.status.online ? 2 : 1"
        :stroke-dasharray="p.status.online ? undefined : '3 5'"
        class="hoverable"
      />
    </svg>
    <div class="move absolute -translate-x-1/2 -translate-y-1/2" :style="{ left: `${width / 2}px`, top: `${height / 2}px` }">
      <DeviceOrb :name="selfName" :os="selfOs" online self />
    </div>
    <div
      v-for="p in placed"
      :key="p.id"
      class="move absolute -translate-x-1/2 -translate-y-1/2"
      :style="{ left: `${p.x}px`, top: `${p.y}px` }"
      :data-peer-id="p.id"
    >
      <DeviceOrb
        :name="p.status.peer.name"
        :os="p.status.peer.os"
        :online="p.status.online"
        :mode="p.status.peer.mode"
        :pending="pendingByPeer[p.id] ?? 0"
        :drop-active="dropTarget === p.id"
        @select="$emit('select', p.id)"
      />
    </div>
  </div>
</template>
