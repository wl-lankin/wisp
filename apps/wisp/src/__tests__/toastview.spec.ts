import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { reactive, ref } from 'vue'
import { applySnapshot, initialState, pushNotice, reduce } from '../lib/store'
import type { Offer } from '../lib/types'

const state = reactive(initialState())
vi.mock('../lib/appStore', () => ({ useAppStore: () => ({ state, settings: ref(null), resync: vi.fn() }) }))
vi.mock('../lib/api', () => ({
  api: { hideWindow: vi.fn().mockResolvedValue(undefined), fitToast: vi.fn().mockResolvedValue(undefined) },
}))

const offer = (itemId: string): Offer => ({
  peer: 'm',
  itemId,
  kind: 'Text',
  size: 1,
  preview: { kind: 'text', text: itemId, smart: { type: 'plain' } },
  ageMs: 0,
})

describe('ToastView', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} unobserve() {} })
    Object.assign(state, { ready: true, platform: 'windows', peers: [], inbox: [offer('1')], notices: [], offerSeq: 0 })
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
    vi.clearAllMocks()
  })

  async function setup() {
    const { api } = await import('../lib/api')
    const { default: ToastView } = await import('../views/ToastView.vue')
    const w = mount(ToastView)
    await flushPromises()
    return { api, w }
  }

  it('lingers 8 s, then hides exactly once', async () => {
    const { api, w } = await setup()
    await vi.advanceTimersByTimeAsync(7000)
    expect(api.hideWindow).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(2000)
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(20000)
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    w.unmount()
  })

  it('stays while hovered', async () => {
    const { api, w } = await setup()
    await w.find('[data-testid="toast-content"]').trigger('mouseenter')
    await vi.advanceTimersByTimeAsync(30000)
    expect(api.hideWindow).not.toHaveBeenCalled()
    w.unmount()
  })

  it('restarts the linger for a new offer after a hide', async () => {
    const { api, w } = await setup()
    await vi.advanceTimersByTimeAsync(9000)
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    state.inbox = [offer('2'), ...state.inbox]
    await flushPromises()
    await vi.advanceTimersByTimeAsync(7000)
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(2000)
    expect(api.hideWindow).toHaveBeenCalledTimes(2)
    w.unmount()
  })

  it('comes back for a repeated offer that is already in the inbox', async () => {
    const { api, w } = await setup()
    await vi.advanceTimersByTimeAsync(9000)
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    reduce(state, { type: 'offerReceived', ...offer('1') })
    await flushPromises()
    expect((w.element as HTMLElement).style.opacity).toBe('1')
    await vi.advanceTimersByTimeAsync(7000)
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(2000)
    expect(api.hideWindow).toHaveBeenCalledTimes(2)
    w.unmount()
  })

  it('shows an engine error and stays until it is dismissed', async () => {
    const { api, w } = await setup()
    state.inbox = []
    pushNotice(state, 'error', "Couldn't write the clipboard. Try again.")
    await flushPromises()
    expect(w.text()).toContain("Couldn't write the clipboard. Try again.")
    await vi.advanceTimersByTimeAsync(30000)
    expect(api.hideWindow).not.toHaveBeenCalled()
    await w.find('[data-testid="toast-error"] button').trigger('click')
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.text()).not.toContain("Couldn't write the clipboard")
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    w.unmount()
  })

  it('comes back to show a clipboard error that arrives while hidden', async () => {
    const { api, w } = await setup()
    await vi.advanceTimersByTimeAsync(9000)
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    expect((w.element as HTMLElement).style.opacity).toBe('0')
    pushNotice(state, 'error', "Couldn't write the clipboard. Try again.")
    await flushPromises()
    expect((w.element as HTMLElement).style.opacity).toBe('1')
    expect(w.text()).toContain("Couldn't write the clipboard. Try again.")
    await vi.advanceTimersByTimeAsync(30000)
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    w.unmount()
  })

  it('shows a clipboard error that happened before this window existed', async () => {
    Object.assign(state, { ready: false, inbox: [] })
    const { api, w } = await setup()
    await vi.advanceTimersByTimeAsync(2000)
    applySnapshot(state, {
      deviceId: 'd',
      deviceName: 'PC',
      platform: 'windows',
      port: 1,
      peers: [],
      nearby: [],
      inbox: [],
      clipboardError: { seq: 1, message: "Couldn't write the clipboard. Try again." },
    })
    await flushPromises()
    expect((w.element as HTMLElement).style.opacity).toBe('1')
    expect(w.text()).toContain("Couldn't write the clipboard. Try again.")
    await vi.advanceTimersByTimeAsync(30000)
    expect(api.hideWindow).not.toHaveBeenCalled()
    w.unmount()
  })

  it('ignores a non-clipboard error that arrived while hidden', async () => {
    const { api, w } = await setup()
    await vi.advanceTimersByTimeAsync(9000)
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    pushNotice(state, 'error', 'Transfer failed')
    await flushPromises()
    expect((w.element as HTMLElement).style.opacity).toBe('0')
    reduce(state, { type: 'offerReceived', ...offer('2') })
    await flushPromises()
    expect(w.text()).not.toContain('Transfer failed')
    await vi.advanceTimersByTimeAsync(9000)
    expect(api.hideWindow).toHaveBeenCalledTimes(2)
    w.unmount()
  })

  it('does not wake again when a newer error is dismissed', async () => {
    const { api, w } = await setup()
    state.inbox = []
    pushNotice(state, 'error', 'First problem')
    pushNotice(state, 'error', 'Second problem')
    await flushPromises()
    await w.find('[data-testid="toast-error"] button').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('First problem')
    await w.find('[data-testid="toast-error"] button').trigger('click')
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.text()).not.toContain('problem')
    expect(api.hideWindow).toHaveBeenCalledTimes(1)
    w.unmount()
  })

  it('shows the shortcut with Mac names on macOS', async () => {
    state.platform = 'macos'
    const { w } = await setup()
    expect(w.text()).toContain('Ctrl+Option+V')
    w.unmount()
  })
})
