export interface OrbPlacement {
  id: string
  x: number
  y: number
}

const ORB_RADIUS = 36

/** Angles in order: left, right, then alternating upper and lower slots on both sides. */
const SLOTS = [180, 0, 135, 45, 225, 315, 90, 270, 160, 20, 200, 340]

export function layoutOrbs(ids: string[], width: number, height: number): OrbPlacement[] {
  const cx = width / 2
  const cy = height / 2
  const rx = Math.max(0, width / 2 - ORB_RADIUS)
  const ry = Math.max(0, height / 2 - ORB_RADIUS)
  return ids.map((id, i) => {
    const ring = Math.floor(i / SLOTS.length)
    const scale = 1 - ring * 0.18
    const angle = ((SLOTS[i % SLOTS.length] + ring * 12) * Math.PI) / 180
    return {
      id,
      x: cx + Math.cos(angle) * rx * scale,
      y: cy - Math.sin(angle) * ry * scale,
    }
  })
}
