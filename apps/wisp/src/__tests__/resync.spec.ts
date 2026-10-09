import { describe, expect, it, vi } from 'vitest'
import { createResyncer, initialState, mergeOffers } from '../lib/store'
import type { Offer } from '../lib/types'

function offer(itemId: string): Offer {
  return { peer: 'p', itemId, kind: 'Text', size: 1, preview: { kind: 'text', text: 'x', smart: { type: 'plain' } }, ageMs: 0 }
}

function deferred<T>() {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((r) => (resolve = r))
  return { promise, resolve }
}

describe('createResyncer', () => {
  it('coalesces rapid requests into at most two fetches', async () => {
    const gates = [deferred<number>(), deferred<number>()]
    const fetch = vi.fn().mockImplementationOnce(() => gates[0].promise).mockImplementationOnce(() => gates[1].promise)
    const applied: number[] = []
    const r = createResyncer<number>(fetch, (n) => applied.push(n), vi.fn())
    const all = Promise.all([r.request(), r.request(), r.request()])
    gates[0].resolve(1)
    await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(2))
    gates[1].resolve(2)
    await all
    expect(fetch).toHaveBeenCalledTimes(2)
    expect(applied).toEqual([1, 2])
  })

  it('never runs two fetches at once, so snapshots apply in order', async () => {
    let active = 0
    let maxActive = 0
    const fetch = vi.fn(async () => {
      active++
      maxActive = Math.max(maxActive, active)
      await new Promise((r) => setTimeout(r, 5))
      active--
      return fetch.mock.calls.length
    })
    const applied: number[] = []
    const r = createResyncer<number>(fetch, (n) => applied.push(n), vi.fn())
    const all = Promise.all([r.request(), r.request()])
    await all
    expect(maxActive).toBe(1)
    expect(applied).toEqual([...applied].sort((a, b) => a - b))
  })

  it('hands offers that arrived during the fetch to apply', async () => {
    const gate = deferred<string>()
    const fetch = vi.fn(() => gate.promise)
    const seen: string[][] = []
    const r = createResyncer<string>(fetch, (_s, offers) => seen.push(offers.map((o) => o.itemId)), vi.fn())
    const done = r.request()
    r.track({ type: 'offerReceived', ...offer('a') })
    r.track({ type: 'offerReceived', ...offer('b') })
    r.track({ type: 'clipboardApplied', itemId: 'a', from: 'p' })
    gate.resolve('snap')
    await done
    expect(seen).toEqual([['b']])
  })

  it('gives up after the attempts, reports once', async () => {
    const fetch = vi.fn().mockRejectedValue(new Error('down'))
    const onGiveUp = vi.fn()
    const r = createResyncer<number>(fetch, vi.fn(), onGiveUp, { attempts: 3, delayMs: 1 })
    await r.request()
    expect(fetch).toHaveBeenCalledTimes(3)
    expect(onGiveUp).toHaveBeenCalledTimes(1)
    fetch.mockReset()
    fetch.mockResolvedValue(7)
    const apply = vi.fn()
    const r2 = createResyncer<number>(fetch, apply, onGiveUp, { attempts: 3, delayMs: 1 })
    await r2.request()
    expect(apply).toHaveBeenCalledWith(7, [], new Set())
    expect(onGiveUp).toHaveBeenCalledTimes(1)
  })

  it('keeps an offer caught during the fetch removed even if the snapshot still has it', async () => {
    const gate = deferred<string>()
    const s = initialState()
    s.inbox = [offer('a'), offer('b')]
    const r = createResyncer<string>(
      () => gate.promise,
      (_snap, offers, removed) => {
        s.inbox = [offer('a'), offer('b')].filter((o) => !removed.has(o.itemId))
        mergeOffers(s, offers)
      },
      vi.fn(),
    )
    const done = r.request()
    r.track({ type: 'clipboardApplied', itemId: 'a', from: 'p' })
    gate.resolve('snap')
    await done
    expect(s.inbox.map((o) => o.itemId)).toEqual(['b'])
  })
})
