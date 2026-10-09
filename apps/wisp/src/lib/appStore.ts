import { reactive, ref } from 'vue'
import { api, onEngineEvent, onSettings } from './api'
import { applySnapshot, createResyncer, initialState, mergeOffers, reduce, type State } from './store'
import type { AppSettings } from './types'

let shared: { state: State; settings: ReturnType<typeof ref<AppSettings | null>>; resync: () => Promise<void> } | null = null

function applyTheme(settings: AppSettings | null) {
  document.documentElement.dataset.theme = settings?.theme ?? 'system'
}

/** The store is per window: each Tauri window has its own JS context, so each builds its own copy. */
export function useAppStore() {
  if (shared) return shared
  const state = reactive(initialState()) as State
  const settings = ref<AppSettings | null>(null)
  applyTheme(null)

  function fail(message: string) {
    reduce(state, { type: 'error', message })
  }

  const resyncer = createResyncer(
    () => api.snapshot(),
    (snap, inFlightOffers, removedIds) => {
      applySnapshot(state, snap)
      state.inbox = state.inbox.filter((o) => !removedIds.has(o.itemId))
      mergeOffers(state, inFlightOffers)
    },
    () => fail('Wisp is not responding. It will retry on the next change.'),
  )
  const resync = () => resyncer.request()

  api
    .getSettings()
    .then((s) => {
      settings.value = s
      applyTheme(s)
    })
    .catch(() => fail('Could not load your settings.'))
  onSettings((s) => {
    const saveDirChanged = settings.value?.saveDir !== s.saveDir
    settings.value = s
    applyTheme(s)
    if (saveDirChanged) void resync()
  }).catch(() => fail('Could not listen for settings changes.'))
  onEngineEvent((e) => {
    resyncer.track(e)
    if (reduce(state, e)) void resync()
  }).catch(() => fail('Could not listen for engine events.'))
  void resync()

  shared = { state, settings, resync }
  return shared
}
