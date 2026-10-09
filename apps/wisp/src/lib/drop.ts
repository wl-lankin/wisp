export function peerIdAt(el: Element | null): string | null {
  return el?.closest<HTMLElement>('[data-peer-id]')?.dataset.peerId ?? null
}

export function toCssPoint(pos: { x: number; y: number }, dpr: number): { x: number; y: number } {
  return { x: pos.x / dpr, y: pos.y / dpr }
}
