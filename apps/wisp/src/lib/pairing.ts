import type { PeerStatus } from './types'

export function normalizeCode(input: string): string | null {
  const digits = input.replace(/\D/g, '')
  return digits.length === 6 ? digits : null
}

export function formatCode(code: string): string {
  return `${code.slice(0, 3)} ${code.slice(3)}`
}

/** Peers whose id is not in `known`, so a resync that replaces the list cannot hide or fake a pairing. */
export function findNewPeers(peers: PeerStatus[], known: Set<string>): PeerStatus[] {
  return peers.filter((p) => !known.has(p.peer.deviceId))
}

/** Turns a raw pairing error into short advice for the person at the keyboard. */
export function friendlyPairingError(e: unknown): string {
  const raw = (e instanceof Error ? e.message : String(e)).replace(/^Error:\s*/, '').trim()
  const text = raw.toLowerCase()
  if (text.includes('too many wrong codes')) return 'Too many wrong codes. Start pairing again on the other device.'
  if (text.includes('wrong code')) return 'That code did not match. Check the digits on the other device and try again.'
  if (text.includes('already paired')) return 'These devices are already paired.'
  if (text.includes('session has ended')) return 'The code expired. Show a new code and try again.'
  if (/timed out|transport|io:|connect|refused|unreachable|network/.test(text)) {
    return 'Could not reach that address. Check the IP address and that Wisp is open on the other device.'
  }
  return raw || 'Pairing did not work. Please try again.'
}
