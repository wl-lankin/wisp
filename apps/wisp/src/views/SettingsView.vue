<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import PairingCard from './PairingCard.vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { comboFromEvent, comboProblem, displayCombo } from '../lib/hotkeys'
import { osLabel } from '../lib/format'
import IconGlyph from '../components/IconGlyph.vue'
import WindowBar from '../components/WindowBar.vue'
import { dismissNotice, pushNotice } from '../lib/store'
import type { AppSettings, DeliveryMode, Theme } from '../lib/types'

const { state, settings, resync } = useAppStore()
const isMac = computed(() => state.platform === 'macos')
const recording = ref(false)
const recorder = ref<HTMLElement | null>(null)
const message = ref('')
const addressInputs = ref<Record<string, string>>({})
const nameInput = ref(state.deviceName)
const renaming = ref(false)
const pendingSize = ref<{ size: number; remove: number } | null>(null)
const sizes = [50, 200, 500, 1000]
let clearTimer: ReturnType<typeof setTimeout> | undefined

function friendly(e: unknown): string {
  const text = e instanceof Error ? e.message : String(e)
  return text.replace(/^Error:\s*/, '')
}

function say(text: string) {
  message.value = text
  clearTimeout(clearTimer)
  clearTimer = setTimeout(() => (message.value = ''), 3000)
}

// Follow the name when it changes elsewhere, unless the owner is typing a new one.
watch(
  () => state.deviceName,
  (now, before) => {
    if (nameInput.value === before) nameInput.value = now
  },
)

const nameReady = computed(() => {
  const name = nameInput.value.trim()
  return name !== '' && name !== state.deviceName
})

async function renameDevice() {
  const name = nameInput.value.trim()
  if (!nameReady.value || renaming.value) return
  renaming.value = true
  try {
    const applied = await api.renameDevice(name)
    state.deviceName = applied
    nameInput.value = applied
    say('Renamed')
  } catch (e) {
    pushNotice(state, 'error', friendly(e))
    say('Not renamed')
  } finally {
    renaming.value = false
  }
}

async function chooseSize(size: number) {
  const current = settings.value?.historySize
  if (current === undefined || size === current) {
    pendingSize.value = null
    return
  }
  if (size > current) {
    pendingSize.value = null
    await save({ historySize: size })
    return
  }
  try {
    const count = await api.historyCount()
    if (count > size) {
      pendingSize.value = { size, remove: count - size }
      return
    }
  } catch (e) {
    pushNotice(state, 'error', friendly(e))
    return
  }
  pendingSize.value = null
  await save({ historySize: size })
}

async function confirmSize() {
  const pending = pendingSize.value
  pendingSize.value = null
  if (pending) await save({ historySize: pending.size })
}

function cancelSize() {
  pendingSize.value = null
}

function toggleScreenshots(e: Event) {
  void save({ shareScreenshotFiles: (e.target as HTMLInputElement).checked })
}

function stopRecording() {
  recording.value = false
}

onMounted(() => window.addEventListener('blur', stopRecording))
onBeforeUnmount(() => {
  clearTimeout(clearTimer)
  window.removeEventListener('blur', stopRecording)
})

async function startRecording() {
  recording.value = true
  await nextTick()
  recorder.value?.focus()
}

/** Saves run one after another; each patches the settings as the previous save left them. */
let saving: Promise<void> = Promise.resolve()

function save(patch: Partial<AppSettings>): Promise<void> {
  const next = saving.then(() => saveNow(patch))
  saving = next
  return next
}

async function saveNow(patch: Partial<AppSettings>) {
  if (!settings.value) return
  try {
    settings.value = await api.updateSettings({ ...settings.value, ...patch })
    say('Saved')
  } catch (e) {
    pushNotice(state, 'error', friendly(e))
    say('Not saved')
  }
}

function record(e: KeyboardEvent) {
  if (!recording.value) return
  e.preventDefault()
  if (e.key === 'Escape') {
    recording.value = false
    return
  }
  const combo = comboFromEvent(e)
  if (!combo) return
  const problem = comboProblem(combo)
  if (problem) {
    say(problem)
    return
  }
  recording.value = false
  void save({ hotkey: combo })
}

function toggleAutostart(e: Event) {
  void save({ autostart: (e.target as HTMLInputElement).checked })
}

async function changeFolder() {
  try {
    const picked = await api.pickFolder()
    if (picked) await save({ saveDir: picked })
  } catch (e) {
    pushNotice(state, 'error', friendly(e))
  }
}

async function changeMode(peer: string, event: Event) {
  const mode = (event.target as HTMLSelectElement).value as DeliveryMode
  try {
    await api.setMode(peer, mode)
    const p = state.peers.find((x) => x.peer.deviceId === peer)
    if (p) p.peer.mode = mode
  } catch (e) {
    pushNotice(state, 'error', friendly(e))
    await resync()
  }
}

async function forgetPeer(peer: string) {
  try {
    await api.forget(peer)
    state.peers = state.peers.filter((x) => x.peer.deviceId !== peer)
  } catch (e) {
    pushNotice(state, 'error', friendly(e))
  }
}

async function addAddress(peer: string) {
  const addr = addressInputs.value[peer]?.trim()
  if (!addr) return
  try {
    await api.addAddress(peer, addr)
    say('Address added, connecting')
    addressInputs.value[peer] = ''
  } catch (e) {
    pushNotice(state, 'error', friendly(e))
  }
}

const themes: { id: Theme; name: string }[] = [
  { id: 'system', name: 'System' },
  { id: 'dark', name: 'Dark' },
  { id: 'light', name: 'Light' },
]
</script>

<template>
  <main class="flex h-screen flex-col gap-6 overflow-y-auto px-6 pb-6 pt-12 text-[13px]" tabindex="-1">
    <WindowBar />
    <header data-tauri-drag-region class="flex items-baseline justify-between">
      <h1 data-tauri-drag-region class="accent-text text-xl font-semibold tracking-tight">Settings</h1>
      <span class="text-[12px] text-[var(--muted)]" role="status">{{ message }}</span>
    </header>

    <ul v-if="state.notices.length" class="flex flex-col gap-1">
      <li v-for="n in state.notices" :key="n.id" class="flex items-start justify-between gap-2 rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2" :class="n.kind === 'error' ? 'text-[var(--danger)]' : 'text-[var(--muted)]'">
        <span>{{ n.text }}</span>
        <button data-testid="dismiss-notice" class="hoverable text-[var(--muted)] hover:text-[var(--text)]" title="Dismiss" @click="dismissNotice(state, n.id)"><IconGlyph name="close" :size="12" /></button>
      </li>
    </ul>

    <section class="flex flex-col gap-2">
      <h2 class="text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">Pair a device</h2>
      <PairingCard />
    </section>

    <section class="flex flex-col gap-2">
      <h2 class="text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">Devices</h2>
      <p v-if="!state.peers.length" class="text-[var(--muted)]">No paired devices yet.</p>
      <article v-for="p in state.peers" :key="p.peer.deviceId" class="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-3">
        <div class="flex items-center justify-between">
          <span class="font-medium">{{ p.peer.name }} <span class="text-[var(--muted)]">{{ osLabel(p.peer.os) }}</span></span>
          <span :class="p.online ? 'text-[var(--accent-a)]' : 'text-[var(--muted)]'">{{ p.online ? 'Online' : 'Offline' }}</span>
        </div>
        <div class="mt-2 flex flex-wrap items-center gap-2">
          <select
            class="rounded-lg border border-[var(--border)] bg-[var(--surface)] px-2 py-1"
            :value="p.peer.mode"
            @change="changeMode(p.peer.deviceId, $event)"
          >
            <option value="Catch">Catch: ask me first</option>
            <option value="AutoSync">Auto-sync: put it on my clipboard</option>
          </select>
          <input v-model="addressInputs[p.peer.deviceId]" class="w-40 rounded-lg border border-[var(--border)] bg-transparent px-2 py-1" placeholder="Add IP address" @keydown.enter="addAddress(p.peer.deviceId)" />
          <button class="hoverable rounded-lg border border-[var(--border)] px-2 py-1 hover:bg-[var(--surface-strong)]" @click="addAddress(p.peer.deviceId)">Add</button>
          <button class="hoverable ml-auto rounded-lg px-2 py-1 text-[var(--danger)] hover:bg-[var(--surface-strong)]" @click="forgetPeer(p.peer.deviceId)">Forget</button>
        </div>
      </article>
    </section>

    <section class="flex flex-col gap-2">
      <h2 class="text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">This device</h2>
      <div class="flex items-center gap-2">
        <input
          v-model="nameInput"
          data-testid="device-name"
          maxlength="64"
          class="w-56 rounded-lg border border-[var(--border)] bg-transparent px-2 py-1"
          aria-label="Device name"
          @keydown.enter="renameDevice"
        />
        <button
          data-testid="rename-save"
          class="hoverable rounded-lg border border-[var(--border)] px-3 py-1 hover:bg-[var(--surface-strong)] disabled:opacity-50"
          :disabled="!nameReady || renaming"
          @click="renameDevice"
        >Save</button>
      </div>
    </section>

    <section v-if="settings" class="flex flex-col gap-3">
      <h2 class="text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">Catch shortcut</h2>
      <div class="flex items-center gap-3">
        <kbd ref="recorder" tabindex="-1" class="rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 font-mono outline-none" :class="recording ? 'border-[var(--accent-a)]' : ''" @keydown="record" @blur="stopRecording">{{ recording ? 'Press keys, Esc to cancel' : displayCombo(settings.hotkey, isMac) }}</kbd>
        <button class="hoverable rounded-lg border border-[var(--border)] px-3 py-1.5 hover:bg-[var(--surface-strong)]" @click="startRecording">Change</button>
      </div>

      <h2 class="mt-2 text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">General</h2>
      <label class="flex items-center gap-2">
        <input type="checkbox" :checked="settings.autostart" @change="toggleAutostart" />
        Start Wisp when I log in
      </label>
      <div class="flex items-center gap-2">
        <span>Appearance</span>
        <button
          v-for="t in themes"
          :key="t.id"
          class="hoverable rounded-full border px-3 py-1"
          :class="settings.theme === t.id ? 'border-[var(--accent-a)]' : 'border-[var(--border)] text-[var(--muted)]'"
          @click="save({ theme: t.id })"
        >{{ t.name }}</button>
      </div>
      <label v-if="isMac" class="flex items-center gap-2">
        <input type="checkbox" data-testid="share-screenshots" :checked="settings.shareScreenshotFiles" @change="toggleScreenshots" />
        Share screenshots saved as files
      </label>
      <div class="flex flex-col gap-2">
        <div class="flex flex-wrap items-center gap-2">
          <span>Timeline size</span>
          <button
            v-for="n in sizes"
            :key="n"
            :data-testid="`size-${n}`"
            class="hoverable rounded-full border px-3 py-1"
            :class="settings.historySize === n ? 'border-[var(--accent-a)]' : 'border-[var(--border)] text-[var(--muted)]'"
            @click="chooseSize(n)"
          >{{ n.toLocaleString('en-US') }} items</button>
        </div>
        <div v-if="pendingSize" class="flex items-center gap-2 rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2" role="alert">
          <span>This removes {{ pendingSize.remove }} older {{ pendingSize.remove === 1 ? 'item' : 'items' }}.</span>
          <button data-testid="size-remove" class="hoverable rounded-lg px-2 py-1 text-[var(--danger)] hover:bg-[var(--surface-strong)]" @click="confirmSize">Remove</button>
          <button data-testid="size-cancel" class="hoverable rounded-lg px-2 py-1 hover:bg-[var(--surface-strong)]" @click="cancelSize">Cancel</button>
        </div>
      </div>
      <div class="flex items-center gap-2">
        <span class="shrink-0">Save files to</span>
        <code class="truncate rounded bg-[var(--surface)] px-2 py-1 font-mono text-[12px]">{{ settings.saveDir ?? 'Downloads/Wisp' }}</code>
        <button class="hoverable rounded-lg border border-[var(--border)] px-3 py-1 hover:bg-[var(--surface-strong)]" @click="changeFolder">Change</button>
      </div>
    </section>

    <section v-if="state.platform === 'windows'" class="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-3 text-[12px]">
      <h2 class="mb-1 font-medium">Windows Firewall</h2>
      <p class="text-[var(--muted)]">When Windows asks whether Wisp may use the network, allow it on private networks. Without that, other devices cannot reach this one.</p>
    </section>

    <footer class="text-[11px] text-[var(--muted)]">Wisp 0.1.0, {{ state.deviceName }} on port {{ state.port }}</footer>
  </main>
</template>
