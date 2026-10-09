<script setup lang="ts">
import IconGlyph from './IconGlyph.vue'

const model = defineModel<string>({ required: true })
defineProps<{ placeholder?: string }>()
</script>

<template>
  <label class="hoverable flex items-center gap-2 rounded-xl border border-[var(--border)] bg-[var(--surface)] px-3 py-2 focus-within:border-[var(--accent-a)]">
    <IconGlyph name="search" class="text-[var(--muted)]" />
    <input
      v-model="model"
      class="w-full bg-transparent text-[13px] outline-none placeholder:text-[var(--muted)]"
      :placeholder="placeholder ?? 'Search'"
      spellcheck="false"
    />
  </label>
</template>
