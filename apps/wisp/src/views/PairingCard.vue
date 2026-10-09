<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, onEngineEvent } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { findNewPeers, formatCode, friendlyPairingError, normalizeCode } from '../lib/pairing'

const { state } = useAppStore()
/** A code that nobody used for this long is cancelled, so it never outlives what the person sees. */
const CODE_LIFETIME_MS = 5 * 60 * 1000
const tab = ref<'show' | 'enter'>('show')
const code = ref<string | null>(null)
const knownPeerIds = ref<Set<string>>(new Set())
const success = ref<string | null>(null)
const error = ref('')
const target = ref('')
const typedAddress = ref('')
const typedCode = ref('')
const pairing = ref(false)
let disposed = false
let showRequest = 0
let expiryTimer: ReturnType<typeof setTimeout> | undefined
let unlisten: UnlistenFn | undefined

function clearExpiry() {
  clearTimeout(expiryTimer)
  expiryTimer = undefined
}

function expire() {
  clearExpiry()
  if (!code.value) return
  code.value = null
  void api.cancelPairing().catch(() => undefined)
  error.value = 'This code expired. Show a new one when you are ready.'
}

const address = computed(() => typedAddress.value.trim() || target.value)

async function showCode() {
  const request = ++showRequest
  error.value = ''
  success.value = null
  knownPeerIds.value = new Set(state.peers.map((p) => p.peer.deviceId))
  try {
    const created = await api.beginPairing()
    if (disposed || request !== showRequest || tab.value !== 'show') {
      // The card went away or moved on while the code was being created.
      void api.cancelPairing().catch(() => undefined)
      return
    }
    code.value = created
    clearExpiry()
    expiryTimer = setTimeout(expire, CODE_LIFETIME_MS)
  } catch (e) {
    if (!disposed && request === showRequest) error.value = friendlyPairingError(e)
  }
}

watch(
  () => state.peers,
  (peers) => {
    if (tab.value !== 'show' || !code.value) return
    const added = findNewPeers(peers, knownPeerIds.value)
    if (added.length === 0) return
    success.value = added[added.length - 1].peer.name
    code.value = null
    clearExpiry()
  },
  { deep: true },
)

function leaveShowTab() {
  showRequest++
  clearExpiry()
  if (code.value) void api.cancelPairing().catch(() => undefined)
  code.value = null
}

watch(tab, (next, prev) => {
  if (prev === 'show') leaveShowTab()
  error.value = ''
  success.value = null
})

async function pair() {
  const c = normalizeCode(typedCode.value)
  if (!c) {
    error.value = 'The code has 6 digits.'
    return
  }
  if (!address.value) {
    error.value = 'Pick a device or type its IP address.'
    return
  }
  pairing.value = true
  error.value = ''
  try {
    const peer = await api.pairWith(address.value, c)
    if (disposed) return
    success.value = peer.name
    typedCode.value = ''
  } catch (e) {
    if (!disposed) error.value = friendlyPairingError(e)
  } finally {
    if (!disposed) pairing.value = false
  }
}

function pick(addr: string | undefined) {
  target.value = addr ?? ''
  typedAddress.value = ''
}

onEngineEvent((e) => {
  if (disposed || e.type !== 'pairingFailed' || tab.value !== 'show' || !code.value) return
  code.value = null
  clearExpiry()
  error.value = friendlyPairingError(e.reason)
})
  .then((off) => {
    if (disposed) off()
    else unlisten = off
  })
  .catch(() => undefined)

/** Ends a live code. Used by the parent when its window is about to close. */
function cancel() {
  leaveShowTab()
}
defineExpose({ cancel })

onBeforeUnmount(() => {
  disposed = true
  unlisten?.()
  leaveShowTab()
})
</script>

<template>
  <div class="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 text-[13px]">
    <div class="mb-4 flex gap-2">
      <button class="hoverable rounded-full border px-3 py-1" :class="tab === 'show' ? 'border-[var(--accent-a)]' : 'border-[var(--border)] text-[var(--muted)]'" @click="tab = 'show'">Show a code</button>
      <button class="hoverable rounded-full border px-3 py-1" :class="tab === 'enter' ? 'border-[var(--accent-a)]' : 'border-[var(--border)] text-[var(--muted)]'" @click="tab = 'enter'">Enter a code</button>
    </div>

    <p v-if="success" class="text-[var(--accent-a)]" role="status">Paired with {{ success }}. You are all set.</p>

    <template v-else-if="tab === 'show'">
      <p class="text-[var(--muted)]">On your other device, open Wisp, choose "Enter a code" and type:</p>
      <p v-if="code" class="accent-text my-4 text-center font-mono text-4xl font-semibold tracking-[0.2em]">{{ formatCode(code) }}</p>
      <p v-if="code" class="text-[11px] text-[var(--muted)]">Waiting for the other device. Three wrong tries end this code.</p>
      <button v-if="!code" class="hoverable mt-2 rounded-lg border border-[var(--border)] px-3 py-1.5" @click="showCode">Show a code</button>
    </template>

    <template v-else>
      <p class="mb-2 text-[var(--muted)]">Pick the device showing the code:</p>
      <div class="mb-3 flex flex-wrap gap-2">
        <button
          v-for="n in state.nearby"
          :key="n.device"
          class="hoverable rounded-full border px-3 py-1"
          :class="target === n.addresses[0] ? 'border-[var(--accent-a)]' : 'border-[var(--border)]'"
          :disabled="!n.addresses.length"
          @click="pick(n.addresses[0])"
        >{{ n.name }}</button>
        <span v-if="!state.nearby.length" class="text-[12px] text-[var(--muted)]">No devices found nearby. Type the IP address instead.</span>
      </div>
      <div class="flex flex-wrap gap-2">
        <input v-model="typedAddress" class="w-44 rounded-lg border border-[var(--border)] bg-transparent px-2 py-1.5" placeholder="or IP address" />
        <input v-model="typedCode" inputmode="numeric" class="w-32 rounded-lg border border-[var(--border)] bg-transparent px-2 py-1.5 font-mono tracking-widest" placeholder="123 456" @keydown.enter="pair" />
        <button
          class="hoverable rounded-lg px-4 py-1.5 font-medium text-[var(--bg)]"
          style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))"
          :disabled="pairing"
          @click="pair"
        >{{ pairing ? 'Pairing' : 'Pair' }}</button>
      </div>
    </template>

    <p v-if="error" class="mt-3 text-[12px] text-[var(--danger)]" role="alert">{{ error }}</p>
  </div>
</template>
