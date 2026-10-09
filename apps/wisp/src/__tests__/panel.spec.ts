import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { reactive, ref } from 'vue'
import { initialState } from '../lib/store'

const state = reactive(initialState())
vi.mock('../lib/appStore', () => ({ useAppStore: () => ({ state, settings: ref(null), resync: vi.fn() }) }))
vi.mock('../lib/api', () => ({
  api: {
    history: vi.fn().mockResolvedValue([]),
    openWindow: vi.fn(),
    hideWindow: vi.fn(),
    sendFiles: vi.fn().mockResolvedValue([]),
    catchItem: vi.fn().mockResolvedValue(undefined),
  },
}))
const dragHandlers: Array<(e: { payload: unknown }) => void> = []
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn((cb: (e: { payload: unknown }) => void) => {
      dragHandlers.push(cb)
      return Promise.resolve(() => {})
    }),
  }),
}))

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

  it('sends dropped files to the peer under the pointer, but not to an offline one', async () => {
    const { api } = await import('../lib/api')
    const peer = (id: string, online: boolean) => ({
      peer: { deviceId: id, name: id, os: 'MacOs', fingerprint: 'x', addresses: [], mode: 'Catch' },
      online,
      addr: null,
    })
    Object.assign(state, { ready: true, peers: [peer('m', true), peer('off', false)], inbox: [], notices: [] })
    const { default: PanelView } = await import('../views/PanelView.vue')
    const w = mount(PanelView, { attachTo: document.body })
    await flushPromises()
    const handler = dragHandlers[dragHandlers.length - 1]
    const orb = document.createElement('div')
    orb.dataset.peerId = 'm'
    const offOrb = document.createElement('div')
    offOrb.dataset.peerId = 'off'
    let hit: Element = orb
    document.elementFromPoint = vi.fn(() => hit)

    handler({ payload: { type: 'over', position: { x: 10, y: 10 } } })
    handler({ payload: { type: 'drop', position: { x: 10, y: 10 }, paths: ['C:\a.txt'] } })
    await flushPromises()
    expect(api.sendFiles).toHaveBeenCalledWith('m', ['C:\a.txt'])

    hit = offOrb
    handler({ payload: { type: 'drop', position: { x: 10, y: 10 }, paths: ['C:\b.txt'] } })
    await flushPromises()
    expect(api.sendFiles).toHaveBeenCalledTimes(1)
    expect(state.notices.some((n) => n.text.includes('off is offline'))).toBe(true)
    w.unmount()
  })

  const mac = { peer: { deviceId: 'm', name: 'MacBook', os: 'MacOs', fingerprint: 'x', addresses: [], mode: 'Catch' }, online: true, addr: null }

  it('drops a caught offer from the list right away', async () => {
    const { api } = await import('../lib/api')
    Object.assign(state, {
      ready: true,
      peers: [mac],
      transfers: {},
      inbox: [{ peer: 'm', itemId: 'i1', kind: 'Text', size: 5, preview: { kind: 'text', text: 'hello', smart: { type: 'plain' } }, ageMs: 0 }],
    })
    const { default: PanelView } = await import('../views/PanelView.vue')
    const w = mount(PanelView)
    await flushPromises()
    await w.findAll('button').filter((b) => b.text() === 'Catch')[0].trigger('click')
    await flushPromises()
    expect(api.catchItem).toHaveBeenCalledWith('i1')
    expect(state.inbox).toEqual([])
    w.unmount()
  })

  it('keeps a failed transfer with its reason until it is dismissed', async () => {
    Object.assign(state, {
      ready: true,
      peers: [mac],
      inbox: [],
      notices: [],
      transfers: { t: { id: 't', peer: 'm', name: 'a.pdf', done: 1, total: 2, outgoing: true, status: 'failed', reason: 'MacBook went offline' } },
    })
    const { default: PanelView } = await import('../views/PanelView.vue')
    const w = mount(PanelView)
    await flushPromises()
    expect(w.text()).toContain('MacBook went offline')
    await w.find('[data-testid="dismiss-transfer"]').trigger('click')
    await flushPromises()
    expect(state.transfers).toEqual({})
    expect(w.text()).not.toContain('MacBook went offline')
    w.unmount()
  })
})
