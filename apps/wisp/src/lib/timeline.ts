import type { ItemKind } from './types'

export interface TimelineFilters {
  text: string
  source: 'all' | string
  kind: 'all' | ItemKind
}

export function historyQuery(f: TimelineFilters): { text?: string; source?: string; kind?: ItemKind; limit: number } {
  const q: { text?: string; source?: string; kind?: ItemKind; limit: number } = { limit: 200 }
  const text = f.text.trim()
  if (text) q.text = text
  if (f.source !== 'all') q.source = f.source
  if (f.kind !== 'all') q.kind = f.kind
  return q
}
