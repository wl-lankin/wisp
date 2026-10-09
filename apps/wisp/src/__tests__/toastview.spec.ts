import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { reactive, ref } from 'vue'
import { initialState } from '../lib/store'
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
    Object.assign(state, { ready: true, platform: 'windows', peers: [], inbox: [offer('1')] })
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
})
