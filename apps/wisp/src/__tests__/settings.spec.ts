import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { reactive, ref } from 'vue'
import { initialState, pushNotice } from '../lib/store'
import type { AppSettings } from '../lib/types'

const state = reactive(initialState())
const settings = ref<AppSettings | null>(null)
vi.mock('../lib/appStore', () => ({ useAppStore: () => ({ state, settings, resync: vi.fn() }) }))
vi.mock('../lib/api', () => ({
  api: {
    updateSettings: vi.fn(),
    beginPairing: vi.fn(),
    cancelPairing: vi.fn().mockResolvedValue(undefined),
    pickFolder: vi.fn(),
  },
  onEngineEvent: vi.fn(() => Promise.resolve(() => {})),
}))

const base: AppSettings = { deviceName: 'PC', saveDir: null, hotkey: 'Ctrl+Alt+V', autostart: false, theme: 'system', onboarded: true }

describe('SettingsView', () => {
  afterEach(() => {
    vi.clearAllMocks()
  })

  it('runs quick saves one after another so none undoes the other', async () => {
    const { api } = await import('../lib/api')
    const update = api.updateSettings as unknown as ReturnType<typeof vi.fn>
    const pending: Array<() => void> = []
    update.mockImplementation((next: AppSettings) => new Promise((r) => pending.push(() => r(next))))
    settings.value = { ...base }
    Object.assign(state, { ready: true, platform: 'windows', peers: [], notices: [] })
    const { default: SettingsView } = await import('../views/SettingsView.vue')
    const w = mount(SettingsView)
    await flushPromises()

    const box = w.find('input[type="checkbox"]')
    ;(box.element as HTMLInputElement).checked = true
    await box.trigger('change')
    await w.findAll('button').filter((b) => b.text() === 'Dark')[0].trigger('click')
    await flushPromises()
    expect(update).toHaveBeenCalledTimes(1)

    pending.shift()?.()
    await flushPromises()
    expect(update).toHaveBeenCalledTimes(2)
    expect(update.mock.calls[1][0]).toMatchObject({ autostart: true, theme: 'dark' })
    pending.shift()?.()
    await flushPromises()
    expect(settings.value).toMatchObject({ autostart: true, theme: 'dark' })
    w.unmount()
  })

  it('lets notices be dismissed', async () => {
    settings.value = { ...base }
    Object.assign(state, { ready: true, platform: 'windows', peers: [], notices: [] })
    pushNotice(state, 'error', 'Could not save')
    const { default: SettingsView } = await import('../views/SettingsView.vue')
    const w = mount(SettingsView)
    await flushPromises()
    expect(w.text()).toContain('Could not save')
    await w.find('[data-testid="dismiss-notice"]').trigger('click')
    expect(state.notices).toEqual([])
    w.unmount()
  })
})
