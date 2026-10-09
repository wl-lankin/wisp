import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'

const win = { minimize: vi.fn(), toggleMaximize: vi.fn(), close: vi.fn() }
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => win }))

import WindowBar from '../components/WindowBar.vue'

const userAgent = (ua: string) => vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(ua)

describe('WindowBar', () => {
  afterEach(() => vi.restoreAllMocks())

  it('draws minimize, maximize and close on Windows and drives the window', async () => {
    userAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')
    const bar = mount(WindowBar, { props: { maximizable: true } })
    expect(bar.attributes('data-tauri-drag-region')).toBeDefined()
    await bar.get('[data-testid=window-minimize]').trigger('click')
    await bar.get('[data-testid=window-maximize]').trigger('click')
    await bar.get('[data-testid=window-close]').trigger('click')
    expect(win.minimize).toHaveBeenCalled()
    expect(win.toggleMaximize).toHaveBeenCalled()
    expect(win.close).toHaveBeenCalled()
  })

  it('leaves out maximize on a fixed-size window', () => {
    userAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')
    const bar = mount(WindowBar)
    expect(bar.find('[data-testid=window-maximize]').exists()).toBe(false)
    expect(bar.find('[data-testid=window-close]').exists()).toBe(true)
  })

  it('draws no buttons on macOS, where the traffic lights stay', () => {
    userAgent('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')
    const bar = mount(WindowBar, { props: { maximizable: true } })
    expect(bar.findAll('button')).toHaveLength(0)
  })
})
