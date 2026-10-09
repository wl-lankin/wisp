<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import Constellation from '../components/Constellation.vue'
import ItemCard from '../components/ItemCard.vue'
import IconGlyph from '../components/IconGlyph.vue'
import SearchField from '../components/SearchField.vue'
import TransferRow from '../components/TransferRow.vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { peerIdAt, toCssPoint } from '../lib/drop'
import { formatAge } from '../lib/format'
import { dismissNotice, pushNotice } from '../lib/store'
import type { HistoryEntry, Os } from '../lib/types'

const { state } = useAppStore()
const selected = ref<string | null>(null)
const dropTarget = ref<string | null>(null)
const search = ref('')
const recent = ref<HistoryEntry[]>([])
const busy = ref(false)
const confirmForget = ref(false)

const selfOs = computed<Os>(() => (state.platform === 'macos' ? 'MacOs' : state.platform === 'windows' ? 'Windows' : 'Linux'))
const selectedPeer = computed(() => state.peers.find((p) => p.peer.deviceId === selected.value) ?? null)
const pendingByPeer = computed(() =>
  state.inbox.reduce<Record<string, number>>((acc, o) => ((acc[o.peer] = (acc[o.peer] ?? 0) + 1), acc), {}),
)
/** Running transfers, plus failed ones that stay with their reason until dismissed. */
const shownTransfers = computed(() => Object.values(state.transfers).filter((t) => t.status !== 'done'))

function nameOf(id: string): string {
  if (id === state.deviceId) return 'This device'
  return state.peers.find((p) => p.peer.deviceId === id)?.peer.name ?? 'Unknown device'
}

let seq = 0
async function loadRecent() {
  const mine = ++seq
  try {
    const rows = await api.history({ text: search.value || undefined, limit: 5 })
    if (mine === seq) recent.value = rows
  } catch {
    if (mine === seq) recent.value = []
  }
}

let searchTimer: ReturnType<typeof setTimeout> | undefined
watch(search, () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(loadRecent, 150)
})
watch(() => [state.historyVersion, state.ready], loadRecent, { immediate: true })
watch(selected, () => (confirmForget.value = false))

async function run(task: () => Promise<unknown>) {
  busy.value = true
  try {
    await task()
  } catch (e) {
    pushNotice(state, 'error', String(e))
  } finally {
    busy.value = false
  }
}

function toggleMode() {
  const p = selectedPeer.value
  if (!p) return
  const next = p.peer.mode === 'Catch' ? 'AutoSync' : 'Catch'
  void run(async () => {
    await api.setMode(p.peer.deviceId, next)
    p.peer.mode = next
  })
}

function catchOffer(itemId: string) {
  void run(async () => {
    await api.catchItem(itemId)
    state.inbox = state.inbox.filter((x) => x.itemId !== itemId)
  })
}

function dismissTransfer(id: string) {
  delete state.transfers[id]
}

function dismissOffer(itemId: string) {
  void run(async () => {
    await api.dismiss(itemId)
    state.inbox = state.inbox.filter((x) => x.itemId !== itemId)
  })
}

function copyItem(id: string) {
  void run(() => api.copyFromHistory(id))
}

function sendDropped(peerId: string, paths: string[]) {
  const target = state.peers.find((x) => x.peer.deviceId === peerId)
  if (target && !target.online) {
    pushNotice(state, 'warning', `${target.peer.name} is offline. Try again when it is back.`)
    return
  }
  void run(() => api.sendFiles(peerId, paths))
}

function sendClipboard() {
  const p = selectedPeer.value
  if (!p) return
  void run(() => api.sendClipboardTo(p.peer.deviceId))
}

function forget() {
  const p = selectedPeer.value
  if (!p) return
  if (!confirmForget.value) {
    confirmForget.value = true
    return
  }
  void run(async () => {
    await api.forget(p.peer.deviceId)
    state.peers = state.peers.filter((x) => x.peer.deviceId !== p.peer.deviceId)
    selected.value = null
  })
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') void api.hideWindow('panel')
}

let unlistenDrop: (() => void) | undefined
let disposed = false
onMounted(async () => {
  window.addEventListener('keydown', onKey)
  try {
    const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      const p = event.payload
      if (p.type === 'leave') {
        dropTarget.value = null
        return
      }
      const pt = toCssPoint(p.position, window.devicePixelRatio || 1)
      const peer = peerIdAt(document.elementFromPoint(pt.x, pt.y))
      if (p.type === 'drop') {
        dropTarget.value = null
        if (peer && p.paths.length) sendDropped(peer, p.paths)
      } else {
        dropTarget.value = peer
      }
    })
    if (disposed) unlisten()
    else unlistenDrop = unlisten
  } catch {
    pushNotice(state, 'warning', 'Dropping files on a device is unavailable right now.')
  }
})
onUnmounted(() => {
  disposed = true
  clearTimeout(searchTimer)
  window.removeEventListener('keydown', onKey)
  unlistenDrop?.()
})
</script>

<template>
  <main class="glass flex h-screen flex-col gap-3 overflow-hidden p-4">
    <header class="flex items-center justify-between">
      <h1 class="accent-text text-lg font-semibold tracking-tight">Wisp</h1>
      <nav class="flex gap-1 text-[var(--muted)]">
        <button class="hoverable rounded-lg p-1.5 hover:bg-[var(--surface-strong)] hover:text-[var(--text)]" title="Timeline" @click="api.openWindow('timeline')">
          <IconGlyph name="timeline" />
        </button>
        <button class="hoverable rounded-lg p-1.5 hover:bg-[var(--surface-strong)] hover:text-[var(--text)]" title="Settings" @click="api.openWindow('settings')">
          <IconGlyph name="gear" />
        </button>
      </nav>
    </header>

    <div v-for="n in state.notices" :key="n.id" class="flex items-start justify-between gap-2 rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-[12px]" :class="n.kind === 'error' ? 'text-[var(--danger)]' : ''">
      <span>{{ n.text }}</span>
      <button class="text-[var(--muted)]" title="Dismiss" @click="dismissNotice(state, n.id)"><IconGlyph name="close" :size="12" /></button>
    </div>

    <section v-if="state.ready && !state.peers.length" class="flex flex-1 flex-col items-center justify-center gap-3 text-center">
      <p class="text-[13px] text-[var(--muted)]">No devices yet. Pair one to start sharing.</p>
      <button class="hoverable rounded-xl px-4 py-2 text-[13px] font-medium text-[var(--bg)]" style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))" @click="api.openWindow('settings')">
        Pair a device
      </button>
    </section>

    <template v-else>
      <div class="flex justify-center">
        <Constellation
          :self-name="state.deviceName || 'This device'"
          :self-os="selfOs"
          :peers="state.peers"
          :pending-by-peer="pendingByPeer"
          :drop-target="dropTarget"
          @select="(id) => (selected = selected === id ? null : id)"
        />
      </div>

      <section v-if="selectedPeer" class="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-3 text-[12px]">
        <div class="mb-2 flex items-center justify-between">
          <span class="font-medium">{{ selectedPeer.peer.name }}</span>
          <span class="text-[var(--muted)]">{{ selectedPeer.online ? 'Online' : 'Offline' }}</span>
        </div>
        <div class="flex flex-wrap gap-2">
          <button class="hoverable rounded-lg border border-[var(--border)] px-2.5 py-1.5 hover:bg-[var(--surface-strong)]" :disabled="busy" @click="toggleMode">
            {{ selectedPeer.peer.mode === 'Catch' ? 'Catch mode' : 'Auto-sync' }}
          </button>
          <button class="hoverable rounded-lg border border-[var(--border)] px-2.5 py-1.5 hover:bg-[var(--surface-strong)]" :disabled="busy || !selectedPeer.online" @click="sendClipboard">
            Send my clipboard
          </button>
          <button class="hoverable rounded-lg px-2.5 py-1.5 text-[var(--danger)] hover:bg-[var(--surface-strong)]" :disabled="busy" @click="forget">
            {{ confirmForget ? 'Click again to forget' : 'Forget' }}
          </button>
        </div>
        <p class="mt-2 text-[11px] text-[var(--muted)]">Drop files on {{ selectedPeer.peer.name }} to send them.</p>
      </section>

      <section v-if="state.inbox.length" class="flex flex-col gap-2">
        <h2 class="text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">Waiting to be caught</h2>
        <ItemCard
          v-for="o in state.inbox.slice(0, 3)"
          :key="o.itemId"
          :preview="o.preview"
          :title="`from ${nameOf(o.peer)}`"
          :meta="formatAge(o.ageMs)"
          compact
        >
          <button class="hoverable rounded-lg px-2.5 py-1 text-[12px] font-medium text-[var(--bg)]" style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))" @click="catchOffer(o.itemId)">Catch</button>
          <button class="hoverable rounded-lg px-2.5 py-1 text-[12px] text-[var(--muted)] hover:text-[var(--text)]" @click="dismissOffer(o.itemId)">Dismiss</button>
        </ItemCard>
      </section>

      <section v-if="shownTransfers.length" class="flex flex-col gap-2">
        <div v-for="t in shownTransfers" :key="t.id" class="flex items-start gap-2">
          <TransferRow class="min-w-0 flex-1" :transfer="t" :peer-name="nameOf(t.peer)" />
          <button
            v-if="t.status === 'failed'"
            data-testid="dismiss-transfer"
            class="hoverable text-[var(--muted)] hover:text-[var(--text)]"
            title="Dismiss"
            @click="dismissTransfer(t.id)"
          >
            <IconGlyph name="close" :size="12" />
          </button>
        </div>
      </section>

      <section class="flex min-h-0 flex-1 flex-col gap-2">
        <SearchField v-model="search" placeholder="Search your timeline" />
        <div class="flex min-h-0 flex-col gap-1.5 overflow-y-auto">
          <button
            v-for="item in recent"
            :key="item.id"
            class="hoverable rounded-lg px-2 py-1.5 text-left hover:bg-[var(--surface-strong)]"
            title="Copy again"
            @click="copyItem(item.id)"
          >
            <ItemCard :preview="item.preview" :title="nameOf(item.source)" :meta="formatAge(Date.now() - item.createdMs)" compact />
          </button>
          <p v-if="!recent.length" class="px-2 text-[12px] text-[var(--muted)]">Nothing here yet. Copy something.</p>
        </div>
      </section>
    </template>
  </main>
</template>
