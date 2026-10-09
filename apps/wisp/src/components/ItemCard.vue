<script setup lang="ts">
import PreviewBody from './PreviewBody.vue'
import type { Preview } from '../lib/types'

defineProps<{ preview: Preview; title: string; meta: string; compact?: boolean }>()
</script>

<template>
  <article class="hoverable rounded-xl border border-[var(--border)] bg-[var(--surface)] p-3 hover:bg-[var(--surface-strong)]">
    <header class="mb-2 flex items-baseline justify-between gap-2 text-[11px]">
      <span class="truncate font-medium">{{ title }}</span>
      <span class="shrink-0 text-[var(--muted)]">{{ meta }}</span>
    </header>
    <PreviewBody :preview="preview" :compact="compact" />
    <footer v-if="$slots.default" class="mt-3 flex gap-2"><slot /></footer>
  </article>
</template>
