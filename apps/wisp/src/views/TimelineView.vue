<script setup lang="ts">
import { computed, onUnmounted, reactive, ref, watch } from 'vue'
import ItemCard from '../components/ItemCard.vue'
import SearchField from '../components/SearchField.vue'
import WindowBar from '../components/WindowBar.vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { formatAge, formatBytes } from '../lib/format'
import { historyQuery, type TimelineFilters } from '../lib/timeline'
import type { HistoryEntry, ItemKind } from '../lib/types'

const { state } = useAppStore()
const filters = reactive<TimelineFilters>({ text: '', source: 'all', kind: 'all' })
const items = ref<HistoryEntry[]>([])
const status = ref('')
const loadFailed = ref(false)

const sources = computed(() => [
  { id: 'all', name: 'All devices' },
  { id: state.deviceId, name: 'This device' },
  ...state.peers.map((p) => ({ id: p.peer.deviceId, name: p.peer.name })),
])
const kinds: { id: 'all' | ItemKind; name: string }[] = [
  { id: 'all', name: 'Everything' },
  { id: 'Text', name: 'Text' },
  { id: 'Html', name: 'Rich text' },
  { id: 'Image', name: 'Images' },
]

function nameOf(id: string) {
  return sources.value.find((s) => s.id === id)?.name ?? 'Unknown device'
}

let seq = 0
async function load() {
  const mine = ++seq
  try {
    const rows = await api.history(historyQuery(filters))
    if (mine !== seq) return
    items.value = rows
    loadFailed.value = false
  } catch {
    if (mine !== seq) return
    items.value = []
    loadFailed.value = true
  }
}

let loadTimer: ReturnType<typeof setTimeout> | undefined
watch(
  () => [filters.text, filters.source, filters.kind, state.historyVersion, state.ready],
  () => {
    clearTimeout(loadTimer)
    loadTimer = setTimeout(load, 120)
  },
  { immediate: true },
)

let statusTimer: ReturnType<typeof setTimeout> | undefined
async function act(task: () => Promise<unknown>, done: string) {
  try {
    await task()
    status.value = done
  } catch (e) {
    status.value = String(e)
  }
  clearTimeout(statusTimer)
  statusTimer = setTimeout(() => (status.value = ''), 2500)
}

function copyItem(id: string) {
  void act(() => api.copyFromHistory(id), 'Copied')
}

function saveItem(id: string) {
  void act(() => api.exportItem(id), 'Saved')
}

onUnmounted(() => {
  clearTimeout(loadTimer)
  clearTimeout(statusTimer)
})
</script>

<template>
  <main class="flex h-screen flex-col gap-4 p-6">
    <WindowBar maximizable class="-mx-6 -mt-6 -mb-2" />
    <header data-tauri-drag-region class="flex flex-wrap items-center gap-4">
      <h1 data-tauri-drag-region class="accent-text text-xl font-semibold tracking-tight">Timeline</h1>
      <div class="w-80"><SearchField v-model="filters.text" placeholder="Search everything you copied" /></div>
      <span class="text-[12px] text-[var(--muted)]">{{ status }}</span>
    </header>
    <nav class="flex flex-wrap gap-2 text-[12px]">
      <button
        v-for="s in sources"
        :key="s.id"
        class="hoverable rounded-full border px-3 py-1"
        :class="filters.source === s.id ? 'border-[var(--accent-a)] text-[var(--text)]' : 'border-[var(--border)] text-[var(--muted)]'"
        @click="filters.source = s.id"
      >{{ s.name }}</button>
      <span class="mx-2 w-px bg-[var(--border)]" />
      <button
        v-for="k in kinds"
        :key="k.id"
        class="hoverable rounded-full border px-3 py-1"
        :class="filters.kind === k.id ? 'border-[var(--accent-b)] text-[var(--text)]' : 'border-[var(--border)] text-[var(--muted)]'"
        @click="filters.kind = k.id"
      >{{ k.name }}</button>
    </nav>
    <section class="grid min-h-0 flex-1 auto-rows-min grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-3 overflow-y-auto pr-1">
      <ItemCard
        v-for="item in items"
        :key="item.id"
        :preview="item.preview"
        :title="nameOf(item.source)"
        :meta="`${formatAge(Date.now() - item.createdMs)}, ${formatBytes(item.size)}`"
      >
        <button class="hoverable rounded-lg border border-[var(--border)] px-2.5 py-1 text-[12px] hover:bg-[var(--surface-strong)]" @click="copyItem(item.id)">Copy</button>
        <button class="hoverable rounded-lg border border-[var(--border)] px-2.5 py-1 text-[12px] hover:bg-[var(--surface-strong)]" @click="saveItem(item.id)">Save as file</button>
      </ItemCard>
      <p v-if="loadFailed" class="col-span-full py-16 text-center text-[13px] text-[var(--danger)]">Could not load your timeline. Try again in a moment.</p>
      <p v-else-if="!items.length" class="col-span-full py-16 text-center text-[13px] text-[var(--muted)]">Nothing matches. Copy something or loosen the filters.</p>
    </section>
  </main>
</template>
