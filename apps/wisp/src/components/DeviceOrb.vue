<script setup lang="ts">
import { computed } from 'vue'
import IconGlyph from './IconGlyph.vue'
import type { DeliveryMode, Os } from '../lib/types'

const props = defineProps<{
  name: string
  os: Os
  online: boolean
  self?: boolean
  mode?: DeliveryMode
  pending?: number
  dropActive?: boolean
}>()
defineEmits<{ select: [] }>()

const glyph = computed(() => (props.os === 'MacOs' ? 'mac' : props.os === 'Windows' ? 'windows' : 'linux'))
</script>

<template>
  <button
    type="button"
    class="orb hoverable group relative flex flex-col items-center gap-1.5 outline-none"
    :class="{ 'opacity-45': !online && !self }"
    :title="name"
    @click="$emit('select')"
  >
    <span
      class="core grid size-[56px] place-items-center rounded-full"
      :class="{ pulse: self, 'is-drop': dropActive }"
    >
      <IconGlyph :name="glyph" :size="22" />
      <span
        v-if="pending"
        class="absolute -right-0.5 top-0 grid min-w-[18px] place-items-center rounded-full px-1 text-[10px] font-semibold text-[var(--bg)]"
        style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))"
      >{{ pending }}</span>
    </span>
    <span class="max-w-[92px] truncate text-[11px] font-medium">{{ name }}</span>
    <span v-if="mode === 'AutoSync'" class="text-[9px] uppercase tracking-wider text-[var(--muted)]">auto</span>
  </button>
</template>

<style scoped>
.core {
  background:
    radial-gradient(circle at 30% 25%, rgb(255 255 255 / 0.22), transparent 55%),
    linear-gradient(140deg, color-mix(in srgb, var(--accent-a) 55%, transparent), color-mix(in srgb, var(--accent-b) 55%, transparent));
  border: 1px solid var(--border);
  color: white;
  transition: box-shadow 160ms ease, transform 160ms ease;
}
.orb:hover .core,
.orb:focus-visible .core {
  box-shadow: var(--glow);
}
.is-drop {
  box-shadow: 0 0 0 3px var(--accent-a), var(--glow);
}
</style>
