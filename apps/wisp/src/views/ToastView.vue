<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import IconGlyph from '../components/IconGlyph.vue'
import PreviewBody from '../components/PreviewBody.vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { formatAge } from '../lib/format'
import { pushNotice } from '../lib/store'
import { groupOffers, shouldHide } from '../lib/toasts'

const { state, settings } = useAppStore()
const root = ref<HTMLElement | null>(null)
const hovering = ref(false)
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

watch(
  () => state.inbox.map((o) => o.itemId).join(','),
  async () => {
    lastChange.value = Date.now()
    await nextTick()
    report()
  },
)

function report() {
  if (root.value) void api.fitToast(root.value.getBoundingClientRect().height + 8)
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
onMounted(() => {
  report()
  timer = setInterval(() => {
    if (state.ready && shouldHide(Date.now(), lastChange.value, hovering.value, state.inbox.length)) {
      void api.hideWindow('toast')
    }
  }, 500)
})
onUnmounted(() => clearInterval(timer))
</script>

<template>
  <div
    ref="root"
    class="flex flex-col gap-2 p-1"
    :class="isMac ? '' : 'justify-end'"
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
