import { reactive, ref } from 'vue'
import { api, onEngineEvent, onSettings } from './api'
import { applySnapshot, initialState, reduce, type State } from './store'
import type { AppSettings } from './types'

let shared: { state: State; settings: ReturnType<typeof ref<AppSettings | null>>; resync: () => Promise<void> } | null = null

function applyTheme(settings: AppSettings | null) {
  document.documentElement.dataset.theme = settings?.theme ?? 'system'
}

export function useAppStore() {
  if (shared) return shared
  const state = reactive(initialState()) as State
  const settings = ref<AppSettings | null>(null)

  async function resync() {
    for (let attempt = 0; attempt < 40; attempt++) {
      try {
        applySnapshot(state, await api.snapshot())
        return
      } catch {
        await new Promise((r) => setTimeout(r, 250))
      }
    }
  }

  void api.getSettings().then((s) => {
    settings.value = s
    applyTheme(s)
  })
  void onSettings((s) => {
    const saveDirChanged = settings.value?.saveDir !== s.saveDir
    settings.value = s
    applyTheme(s)
    if (saveDirChanged) void resync()
  })
  void onEngineEvent((e) => {
    if (reduce(state, e)) void resync()
  })
  void resync()

  shared = { state, settings, resync }
  return shared
}
