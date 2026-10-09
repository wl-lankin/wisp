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
    renameDevice: vi.fn(),
    historyCount: vi.fn(),
  },
  onEngineEvent: vi.fn(() => Promise.resolve(() => {})),
}))

const base: AppSettings = { deviceName: 'PC', saveDir: null, hotkey: 'Ctrl+Alt+V', autostart: false, theme: 'system', onboarded: true, historySize: 200, shareScreenshotFiles: true }

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

  async function mountView(platform = 'windows') {
    settings.value = { ...base }
    Object.assign(state, { ready: true, platform, deviceName: 'PC', peers: [], notices: [] })
    const { default: SettingsView } = await import('../views/SettingsView.vue')
    const w = mount(SettingsView)
    await flushPromises()
    return w
  }

  it('renames this device with the trimmed name and blocks an empty or unchanged name', async () => {
    const { api } = await import('../lib/api')
    const rename = api.renameDevice as unknown as ReturnType<typeof vi.fn>
    rename.mockResolvedValue('Desk')
    const w = await mountView()
    const input = w.find('[data-testid="device-name"]')
    const save = w.find('[data-testid="rename-save"]')
    expect((input.element as HTMLInputElement).value).toBe('PC')
    expect(save.attributes('disabled')).toBeDefined()
    await input.setValue('   ')
    expect(save.attributes('disabled')).toBeDefined()
    await input.setValue('  Desk  ')
    expect(save.attributes('disabled')).toBeUndefined()
    await save.trigger('click')
    await flushPromises()
    expect(rename).toHaveBeenCalledWith('Desk')
    expect(w.text()).toContain('Renamed')
    w.unmount()
  })

  it('does nothing when Enter is pressed on an unchanged or empty name', async () => {
    const { api } = await import('../lib/api')
    const rename = api.renameDevice as unknown as ReturnType<typeof vi.fn>
    rename.mockClear()
    const w = await mountView()
    const input = w.find('[data-testid="device-name"]')
    await input.trigger('keydown.enter')
    await input.setValue('   ')
    await input.trigger('keydown.enter')
    await flushPromises()
    expect(rename).not.toHaveBeenCalled()
    w.unmount()
  })

  it('says "1 older item" in the singular', async () => {
    const { api } = await import('../lib/api')
    ;(api.historyCount as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(51)
    const w = await mountView()
    await w.find('[data-testid="size-50"]').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('This removes 1 older item.')
    expect(w.text()).not.toContain('1 older items')
    w.unmount()
  })

  it('asks before shrinking the timeline and saves only after Remove', async () => {
    const { api } = await import('../lib/api')
    const update = api.updateSettings as unknown as ReturnType<typeof vi.fn>
    update.mockImplementation((next: AppSettings) => Promise.resolve(next))
    ;(api.historyCount as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(120)
    const w = await mountView()
    await w.find('[data-testid="size-50"]').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('This removes 70 older items.')
    expect(update).not.toHaveBeenCalled()
    await w.find('[data-testid="size-remove"]').trigger('click')
    await flushPromises()
    expect(update).toHaveBeenCalledTimes(1)
    expect(update.mock.calls[0][0]).toMatchObject({ historySize: 50 })
    expect(w.text()).not.toContain('This removes')
    w.unmount()
  })

  it('cancels a shrink without saving', async () => {
    const { api } = await import('../lib/api')
    const update = api.updateSettings as unknown as ReturnType<typeof vi.fn>
    ;(api.historyCount as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(120)
    const w = await mountView()
    await w.find('[data-testid="size-50"]').trigger('click')
    await flushPromises()
    await w.find('[data-testid="size-cancel"]').trigger('click')
    expect(update).not.toHaveBeenCalled()
    expect(w.text()).not.toContain('This removes')
    w.unmount()
  })

  it('saves a larger timeline size immediately', async () => {
    const { api } = await import('../lib/api')
    const update = api.updateSettings as unknown as ReturnType<typeof vi.fn>
    update.mockImplementation((next: AppSettings) => Promise.resolve(next))
    const w = await mountView()
    await w.find('[data-testid="size-1000"]').trigger('click')
    await flushPromises()
    expect(update.mock.calls[0][0]).toMatchObject({ historySize: 1000 })
    w.unmount()
  })

  it('shows the screenshot toggle on macOS only', async () => {
    const win = await mountView('windows')
    expect(win.text()).not.toContain('Share screenshots saved as files')
    win.unmount()
    const mac = await mountView('macos')
    expect(mac.text()).toContain('Share screenshots saved as files')
    mac.unmount()
  })
})
