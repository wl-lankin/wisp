import { describe, expect, it } from 'vitest'
import { comboFromEvent, displayCombo } from '../lib/hotkeys'

const ev = (code: string, mods: Partial<Record<'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey', boolean>> = {}) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
})

describe('hotkeys', () => {
  it('builds combos in a fixed modifier order', () => {
    expect(comboFromEvent(ev('KeyV', { ctrlKey: true, altKey: true }))).toBe('Ctrl+Alt+V')
    expect(comboFromEvent(ev('Space', { metaKey: true, shiftKey: true }))).toBe('Shift+Super+Space')
    expect(comboFromEvent(ev('Digit5', { ctrlKey: true }))).toBe('Ctrl+5')
    expect(comboFromEvent(ev('F9', { altKey: true }))).toBe('Alt+F9')
  })
  it('needs a modifier and a real key', () => {
    expect(comboFromEvent(ev('KeyV'))).toBeNull()
    expect(comboFromEvent(ev('ControlLeft', { ctrlKey: true }))).toBeNull()
    expect(comboFromEvent(ev('Comma', { ctrlKey: true }))).toBeNull()
  })
  it('uses Mac names on macOS', () => {
    expect(displayCombo('Ctrl+Alt+V', true)).toBe('Ctrl+Option+V')
    expect(displayCombo('Super+Shift+V', true)).toBe('Cmd+Shift+V')
    expect(displayCombo('Ctrl+Alt+V', false)).toBe('Ctrl+Alt+V')
  })
})
