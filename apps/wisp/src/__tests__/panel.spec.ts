import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { reactive, ref } from 'vue'
import { initialState } from '../lib/store'

const state = reactive(initialState())
vi.mock('../lib/appStore', () => ({ useAppStore: () => ({ state, settings: ref(null), resync: vi.fn() }) }))
vi.mock('../lib/api', () => ({
  api: { history: vi.fn().mockResolvedValue([]), openWindow: vi.fn(), hideWindow: vi.fn() },
}))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: vi.fn().mockResolvedValue(() => {}) }) }))

describe('PanelView', () => {
  it('invites pairing when there are no devices', async () => {
    Object.assign(state, { ready: true, deviceName: 'PC', platform: 'windows', peers: [] })
    const { default: PanelView } = await import('../views/PanelView.vue')
    const w = mount(PanelView)
    await flushPromises()
    expect(w.text()).toContain('Pair a device')
  })

  it('lists offers waiting to be caught', async () => {
    Object.assign(state, {
      ready: true,
      peers: [{ peer: { deviceId: 'm', name: 'MacBook', os: 'MacOs', fingerprint: 'x', addresses: [], mode: 'Catch' }, online: true, addr: null }],
      inbox: [{ peer: 'm', itemId: 'i1', kind: 'Text', size: 5, preview: { kind: 'text', text: 'hello', smart: { type: 'plain' } }, ageMs: 0 }],
    })
    const { default: PanelView } = await import('../views/PanelView.vue')
    const w = mount(PanelView)
    await flushPromises()
    expect(w.text()).toContain('Waiting to be caught')
    expect(w.text()).toContain('hello')
    expect(w.text()).toContain('MacBook')
  })
})
