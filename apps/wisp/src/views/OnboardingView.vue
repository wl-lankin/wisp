<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import PairingCard from './PairingCard.vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'

const { state } = useAppStore()
const step = ref<'name' | 'pair'>('name')
const name = ref('')
const error = ref('')
const busy = ref(false)
const card = ref<InstanceType<typeof PairingCard> | null>(null)
let disposed = false

function friendly(e: unknown): string {
  const text = e instanceof Error ? e.message : String(e)
  return text.replace(/^Error:\s*/, '')
}

/** The backend platform once the engine is up, otherwise the browser's own idea of it. */
function onWindows(): boolean {
  return state.platform ? state.platform === 'windows' : navigator.userAgent.includes('Windows')
}

onMounted(async () => {
  try {
    const suggested = await api.defaultDeviceName()
    if (!disposed && !name.value) name.value = suggested
  } catch {
    // The field stays empty and the person types a name.
  }
})
onBeforeUnmount(() => {
  disposed = true
})

async function next() {
  if (busy.value || !name.value.trim()) return
  busy.value = true
  error.value = ''
  try {
    await api.completeOnboarding(name.value)
    if (!disposed) step.value = 'pair'
  } catch (e) {
    if (!disposed) error.value = friendly(e)
  } finally {
    if (!disposed) busy.value = false
  }
}

async function done() {
  error.value = ''
  card.value?.cancel()
  try {
    await api.openWindow('panel')
    await api.hideWindow('onboarding')
  } catch (e) {
    error.value = friendly(e)
  }
}
</script>

<template>
  <main class="glass flex h-screen flex-col gap-5 p-8">
    <header data-tauri-drag-region class="flex items-center justify-between">
      <h1 class="accent-text text-2xl font-semibold tracking-tight" data-tauri-drag-region>Welcome to Wisp</h1>
      <span class="text-[12px] text-[var(--muted)]">{{ step === 'name' ? '1 of 2' : '2 of 2' }}</span>
    </header>

    <section v-if="step === 'name'" class="flex flex-1 flex-col gap-4">
      <p class="text-[14px] text-[var(--muted)]">Your clipboard, on every computer you own. First, what should other devices call this one?</p>
      <input
        v-model="name"
        maxlength="64"
        class="rounded-xl border border-[var(--border)] bg-[var(--surface)] px-4 py-3 text-[15px] outline-none focus:border-[var(--accent-a)]"
        @keydown.enter="next"
      />
      <p v-if="error" class="text-[12px] text-[var(--danger)]" role="alert">{{ error }}</p>
      <button
        class="hoverable mt-auto self-end rounded-xl px-5 py-2.5 font-medium text-[var(--bg)]"
        style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))"
        :disabled="busy || !name.trim()"
        @click="next"
      >Continue</button>
    </section>

    <section v-else class="flex flex-1 flex-col gap-4 overflow-y-auto">
      <p class="text-[14px] text-[var(--muted)]">Now connect your other computer. Install Wisp there too, then pair with a code.</p>
      <PairingCard ref="card" />
      <p v-if="onWindows()" class="text-[12px] text-[var(--muted)]">If Windows asks whether Wisp may use the network, allow it on private networks.</p>
      <p v-if="error" class="text-[12px] text-[var(--danger)]" role="alert">{{ error }}</p>
      <button class="hoverable mt-auto self-end rounded-xl border border-[var(--border)] px-5 py-2.5 hover:bg-[var(--surface-strong)]" @click="done">Done</button>
    </section>
  </main>
</template>
