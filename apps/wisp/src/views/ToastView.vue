<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import IconGlyph from '../components/IconGlyph.vue'
import PreviewBody from '../components/PreviewBody.vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { formatAge } from '../lib/format'
import { pushNotice } from '../lib/store'
import { groupOffers, shouldHide } from '../lib/toasts'

const { state, settings } = useAppStore()
const content = ref<HTMLElement | null>(null)
const hovering = ref(false)
const hidden = ref(false)
const lastChange = ref(Date.now())
const working = ref<string | null>(null)

const isMac = computed(() => state.platform === 'macos')
const hotkeyHint = computed(() => {
  const combo = settings.value?.hotkey ?? 'Ctrl+Alt+V'
  return isMac.value ? combo.replace('Alt', 'Option') : combo
})

const groups = computed(() =>
  groupOffers(state.inbox, (id) => state.peers.find((p) => p.peer.deviceId === id)?.peer.name ?? 'A device'),
)

const FADE_MS = 200
let fadeTimer: ReturnType<typeof setTimeout> | undefined
let lastHeight = 0

watch(
  () => state.inbox.map((o) => o.itemId).join(','),
  () => {
    hovering.value = false
    lastChange.value = Date.now()
    if (state.inbox.length > 0) {
      clearTimeout(fadeTimer)
      hidden.value = false
    }
  },
)

function report() {
  if (!content.value) return
  const height = Math.ceil(content.value.getBoundingClientRect().height) + 8
  if (height === lastHeight) return
  lastHeight = height
  void api.fitToast(height)
}

function hideSelf() {
  hovering.value = false
  hidden.value = true
  fadeTimer = setTimeout(() => void api.hideWindow('toast'), FADE_MS)
}

function leave() {
  hovering.value = false
  lastChange.value = Date.now()
}

const tasks = {
  dismiss: (id: string) => api.dismiss(id),
  catch: (id: string) => api.catchItem(id),
  save: (id: string) => api.saveOffer(id),
}

async function run(itemId: string, action: keyof typeof tasks) {
  working.value = itemId
  try {
    await tasks[action](itemId)
    state.inbox = state.inbox.filter((o) => o.itemId !== itemId)
  } catch (e) {
    pushNotice(state, 'error', String(e))
  } finally {
    working.value = null
  }
}

let timer: ReturnType<typeof setInterval> | undefined
let observer: ResizeObserver | undefined

function releaseHover() {
  hovering.value = false
}
function onVisibility() {
  if (document.visibilityState !== 'visible') releaseHover()
}

onMounted(() => {
  if (content.value) {
    observer = new ResizeObserver(report)
    observer.observe(content.value)
  }
  report()
  window.addEventListener('blur', releaseHover)
  document.addEventListener('visibilitychange', onVisibility)
  timer = setInterval(() => {
    if (state.ready && !hidden.value && shouldHide(Date.now(), lastChange.value, hovering.value, state.inbox.length)) {
      hideSelf()
    }
  }, 500)
})
onUnmounted(() => {
  clearInterval(timer)
  clearTimeout(fadeTimer)
  observer?.disconnect()
  window.removeEventListener('blur', releaseHover)
  document.removeEventListener('visibilitychange', onVisibility)
})
</script>

<template>
  <div
    class="flex min-h-screen flex-col transition-opacity duration-200"
    :class="isMac ? 'justify-start' : 'justify-end'"
    :style="{ opacity: hidden ? 0 : 1 }"
  >
    <div
      ref="content"
      data-testid="toast-content"
      class="flex flex-col gap-2 p-1"
      @mouseenter="hovering = true"
      @mouseleave="leave"
    >
    <TransitionGroup name="toast">
      <article v-for="g in groups" :key="g.peer" class="glass p-3 shadow-2xl">
        <header class="mb-2 flex items-center justify-between text-[11px]">
          <span class="font-medium">from {{ g.name }}</span>
          <span class="flex items-center gap-2 text-[var(--muted)]">
            {{ formatAge(g.offers[0].ageMs) }}
            <button
              title="Dismiss"
              class="hoverable hover:text-[var(--text)]"
              @click="run(g.offers[0].itemId, 'dismiss')"
            >
              <IconGlyph name="close" :size="12" />
            </button>
          </span>
        </header>
        <PreviewBody :preview="g.offers[0].preview" compact />
        <p v-if="g.offers.length > 1" class="mt-1 text-[11px] text-[var(--muted)]">
          +{{ g.offers.length - 1 }} more from {{ g.name }}
        </p>
        <footer class="mt-3 flex items-center gap-2">
          <button
            class="hoverable rounded-lg px-3 py-1.5 text-[12px] font-semibold text-[var(--bg)]"
            style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))"
            :disabled="working !== null"
            @click="run(g.offers[0].itemId, 'catch')"
          >
            Catch
          </button>
          <button
            class="hoverable rounded-lg border border-[var(--border)] px-3 py-1.5 text-[12px] hover:bg-[var(--surface-strong)]"
            :disabled="working !== null"
            @click="run(g.offers[0].itemId, 'save')"
          >
            Save as file
          </button>
          <span class="ml-auto font-mono text-[10px] text-[var(--muted)]">{{ hotkeyHint }}</span>
        </footer>
      </article>
    </TransitionGroup>
    </div>
  </div>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: opacity 200ms ease, transform 320ms var(--ease-spring);
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(12px);
}
@media (prefers-reduced-motion: reduce) {
  .toast-enter-from,
  .toast-leave-to {
    transform: none;
  }
}
</style>
