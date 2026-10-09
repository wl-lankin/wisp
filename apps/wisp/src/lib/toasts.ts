import type { Offer } from './types'

export const TOAST_LINGER_MS = 8000

export interface ToastGroup {
  peer: string
  name: string
  offers: Offer[]
}

export function groupOffers(offers: Offer[], nameOf: (id: string) => string, max = 3): ToastGroup[] {
  const groups: ToastGroup[] = []
  for (const offer of offers) {
    let group = groups.find((g) => g.peer === offer.peer)
    if (!group) {
      group = { peer: offer.peer, name: nameOf(offer.peer), offers: [] }
      groups.push(group)
    }
    group.offers.push(offer)
  }
  return groups.slice(0, max)
}

export function shouldHide(now: number, lastChange: number, hovering: boolean, count: number): boolean {
  if (count === 0) return true
  if (hovering) return false
  return now - lastChange >= TOAST_LINGER_MS
}
