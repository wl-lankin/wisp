type KeyLike = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>

function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3)
  if (/^Digit[0-9]$/.test(code)) return code.slice(5)
  if (/^F([1-9]|1[0-2])$/.test(code)) return code
  if (code === 'Space') return 'Space'
  return null
}

export function comboFromEvent(e: KeyLike): string | null {
  const key = keyName(e.code)
  const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super'].filter(Boolean)
  if (!key || mods.length === 0) return null
  return [...mods, key].join('+')
}

export function displayCombo(combo: string, isMac: boolean): string {
  if (!isMac) return combo
  return combo
    .split('+')
    .map((part) => (part === 'Alt' ? 'Option' : part === 'Super' ? 'Cmd' : part))
    .join('+')
}

const EDITING_KEYS = new Set(['C', 'V', 'X', 'A', 'Z'])

/** Returns a friendly reason when a combo should not become a global shortcut. */
export function comboProblem(combo: string): string | null {
  const parts = combo.split('+')
  const key = parts[parts.length - 1]
  const mods = parts.slice(0, -1)
  if (mods.length === 1 && mods[0] === 'Shift') return 'Add Ctrl, Alt or Cmd, Shift alone is not enough. Pick another one.'
  if (mods.length === 1 && (mods[0] === 'Ctrl' || mods[0] === 'Super') && EDITING_KEYS.has(key)) {
    return 'That shortcut is used for editing. Pick another one.'
  }
  return null
}
