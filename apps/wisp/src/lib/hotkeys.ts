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
