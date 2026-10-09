import { describe, expect, it } from 'vitest'
import { comboFromEvent, comboProblem, displayCombo } from '../lib/hotkeys'

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
  it('rejects shift-only and editing combos with a friendly message', () => {
    expect(comboProblem('Shift+V')).toBe('Add Ctrl, Alt or Cmd, Shift alone is not enough. Pick another one.')
    expect(comboProblem('Ctrl+V')).toBe('That shortcut is used for editing. Pick another one.')
    expect(comboProblem('Super+C')).toBe('That shortcut is used for editing. Pick another one.')
    expect(comboProblem('Ctrl+Z')).toBe('That shortcut is used for editing. Pick another one.')
    expect(comboProblem('Ctrl+Alt+V')).toBeNull()
    expect(comboProblem('Ctrl+Shift+V')).toBeNull()
    expect(comboProblem('Ctrl+5')).toBeNull()
    expect(comboProblem('Alt+F9')).toBeNull()
  })
})
