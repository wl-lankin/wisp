import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { reactive, ref } from 'vue'
import { initialState } from '../lib/store'
import type { EngineEvent, PeerStatus } from '../lib/types'

const state = reactive(initialState())
let emit: (e: EngineEvent) => void = () => {}
const unlisten = vi.fn()

vi.mock('../lib/appStore', () => ({ useAppStore: () => ({ state, settings: ref(null), resync: vi.fn() }) }))
vi.mock('../lib/api', () => ({
  api: {
    beginPairing: vi.fn(),
    cancelPairing: vi.fn(),
    pairWith: vi.fn(),
    defaultDeviceName: vi.fn(),
    completeOnboarding: vi.fn(),
    openWindow: vi.fn(),
    hideWindow: vi.fn(),
  },
  onEngineEvent: vi.fn((cb: (e: EngineEvent) => void) => {
    emit = cb
    return Promise.resolve(unlisten)
  }),
}))

function peer(deviceId: string, name: string): PeerStatus {
  return { peer: { deviceId, name, os: 'MacOs', fingerprint: 'ab', addresses: [], mode: 'Catch' }, online: true, addr: null }
}

async function load() {
  const { api } = await import('../lib/api')
  const { default: PairingCard } = await import('../views/PairingCard.vue')
  return { api: api as unknown as Record<string, ReturnType<typeof vi.fn>>, PairingCard }
}

const buttonsByText = (w: ReturnType<typeof mount>, text: string) => w.findAll('button').filter((b) => b.text() === text)
/** The tab, or the action button that arms a code (the last "Show a code" on the page). */
const buttonByText = (w: ReturnType<typeof mount>, text: string) => buttonsByText(w, text)[0]
const actionButton = (w: ReturnType<typeof mount>) => buttonsByText(w, 'Show a code').slice(-1)[0]

describe('PairingCard', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    Object.assign(state, { ready: true, platform: 'windows', peers: [peer('a', 'Old')], nearby: [] })
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.clearAllMocks()
  })

  async function armed(code = '123456') {
    const { api, PairingCard } = await load()
    api.beginPairing.mockResolvedValue(code)
    api.cancelPairing.mockResolvedValue(undefined)
    const w = mount(PairingCard)
    await flushPromises()
    expect(api.beginPairing).not.toHaveBeenCalled()
    await actionButton(w).trigger('click')
    await flushPromises()
    return { api, w }
  }

  it('does not arm a code on mount, only on click', async () => {
    const { api, w } = await armed()
    expect(api.beginPairing).toHaveBeenCalledTimes(1)
    expect(w.text()).toContain('123 456')
    w.unmount()
  })

  it('cancels the code on tab switch and on unmount', async () => {
    const { api, w } = await armed()
    await buttonByText(w, 'Enter a code').trigger('click')
    await flushPromises()
    expect(api.cancelPairing).toHaveBeenCalledTimes(1)
    await buttonByText(w, 'Show a code').trigger('click')
    await flushPromises()
    expect(api.beginPairing).toHaveBeenCalledTimes(1)
    await actionButton(w).trigger('click')
    await flushPromises()
    expect(api.beginPairing).toHaveBeenCalledTimes(2)
    api.cancelPairing.mockClear()
    w.unmount()
    expect(api.cancelPairing).toHaveBeenCalledTimes(1)
  })

  it('asks for one code even when the button is clicked twice quickly', async () => {
    const { api, PairingCard } = await load()
    let resolve: (c: string) => void = () => {}
    api.beginPairing.mockReturnValue(new Promise<string>((r) => (resolve = r)))
    api.cancelPairing.mockResolvedValue(undefined)
    const w = mount(PairingCard)
    await flushPromises()
    await actionButton(w).trigger('click')
    await actionButton(w).trigger('click')
    expect(api.beginPairing).toHaveBeenCalledTimes(1)
    resolve('111222')
    await flushPromises()
    expect(w.text()).toContain('111 222')
    w.unmount()
  })

  it('cancels a code that arrives after unmount', async () => {
    const { api, PairingCard } = await load()
    let resolve: (c: string) => void = () => {}
    api.beginPairing.mockReturnValue(new Promise<string>((r) => (resolve = r)))
    api.cancelPairing.mockResolvedValue(undefined)
    const w = mount(PairingCard)
    await flushPromises()
    await actionButton(w).trigger('click')
    w.unmount()
    api.cancelPairing.mockClear()
    resolve('654321')
    await flushPromises()
    expect(api.cancelPairing).toHaveBeenCalledTimes(1)
  })

  it('reports success when a new peer id appears, even if the list is replaced', async () => {
    const { w } = await armed()
    state.peers = [peer('b', 'MacBook')]
    await flushPromises()
    expect(w.text()).toContain('Paired with MacBook')
    expect(w.text()).not.toContain('123 456')
    w.unmount()
  })

  it('clears a dead code and explains when the core ends the session', async () => {
    const { w } = await armed()
    emit({ type: 'pairingFailed', reason: 'Too many wrong codes. Start pairing again.' })
    await flushPromises()
    expect(w.text()).not.toContain('123 456')
    expect(w.text()).toMatch(/too many wrong codes/i)
    expect(buttonsByText(w, 'Show a code')).toHaveLength(2)
    w.unmount()
    expect(unlisten).toHaveBeenCalled()
  })

  it('expires an armed code after five minutes', async () => {
    const { api, w } = await armed()
    await vi.advanceTimersByTimeAsync(4 * 60 * 1000)
    expect(w.text()).toContain('123 456')
    await vi.advanceTimersByTimeAsync(61 * 1000)
    expect(api.cancelPairing).toHaveBeenCalledTimes(1)
    expect(w.text()).not.toContain('123 456')
    expect(w.text()).toContain('This code expired. Show a new one when you are ready.')
    w.unmount()
  })
})

describe('OnboardingView', () => {
  beforeEach(() => {
    Object.assign(state, { ready: true, platform: 'windows', peers: [], nearby: [] })
  })
  afterEach(() => vi.clearAllMocks())

  it('names the device, continues to pairing, and Done cancels the code before hiding', async () => {
    const { api } = await load()
    const { default: OnboardingView } = await import('../views/OnboardingView.vue')
    const order: string[] = []
    api.defaultDeviceName.mockResolvedValue('DESKTOP-1')
    api.completeOnboarding.mockResolvedValue(undefined)
    api.beginPairing.mockResolvedValue('123456')
    api.openWindow.mockResolvedValue(undefined)
    api.cancelPairing.mockImplementation(async () => void order.push('cancel'))
    api.hideWindow.mockImplementation(async () => void order.push('hide'))
    const w = mount(OnboardingView)
    await flushPromises()
    expect((w.find('input').element as HTMLInputElement).value).toBe('DESKTOP-1')
    await buttonByText(w, 'Continue').trigger('click')
    await flushPromises()
    expect(api.completeOnboarding).toHaveBeenCalledWith('DESKTOP-1')
    expect(w.text()).toContain('2 of 2')
    expect(api.beginPairing).not.toHaveBeenCalled()
    await actionButton(w).trigger('click')
    await flushPromises()
    await buttonByText(w, 'Done').trigger('click')
    await flushPromises()
    expect(order).toEqual(['cancel', 'hide'])
    expect(api.openWindow).toHaveBeenCalledWith('panel')
    expect(api.hideWindow).toHaveBeenCalledWith('onboarding')
    w.unmount()
  })
})
