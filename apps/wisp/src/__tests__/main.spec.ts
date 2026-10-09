import { describe, expect, it, vi } from 'vitest'
import { reactive, ref } from 'vue'
import { initialState } from '../lib/store'

vi.mock('../styles.css', () => ({}))
// Mounting the default view must not start the live store and its snapshot retry loop.
vi.mock('../lib/appStore', () => ({
  useAppStore: () => ({ state: reactive(initialState()), settings: ref(null), resync: vi.fn() }),
}))
vi.mock('../lib/api', () => ({
  api: new Proxy({}, { get: () => vi.fn().mockResolvedValue([]) }),
  onEngineEvent: vi.fn(() => Promise.resolve(() => {})),
  onSettings: vi.fn(() => Promise.resolve(() => {})),
}))
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn(() => Promise.resolve(() => {})) }),
}))

describe('viewFor', () => {
  it('picks the view from the query and falls back to the panel', async () => {
    document.body.innerHTML = '<div id="app"></div>'
    const { viewFor, views } = await import('../main')
    expect(viewFor('?view=timeline')).toBe(views.timeline)
    expect(viewFor('?view=nope')).toBe(views.panel)
    expect(viewFor('')).toBe(views.panel)
  })
})
