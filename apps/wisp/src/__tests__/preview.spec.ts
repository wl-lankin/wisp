import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import PreviewBody from '../components/PreviewBody.vue'

describe('PreviewBody', () => {
  it('shows a swatch for colors', () => {
    const w = mount(PreviewBody, { props: { preview: { kind: 'text', text: '#2DE2C5', smart: { type: 'color', value: '#2DE2C5' } } } })
    expect(w.find('[data-swatch]').attributes('style')).toContain('#2DE2C5')
  })
  it('highlights code', () => {
    const w = mount(PreviewBody, { props: { preview: { kind: 'text', text: 'const a = 1;\nconst b = 2;', smart: { type: 'code' } } } })
    expect(w.find('pre code').html()).toContain('hljs')
  })
  it('shows the domain for links', () => {
    const w = mount(PreviewBody, { props: { preview: { kind: 'text', text: 'https://example.org/a', smart: { type: 'link', domain: 'example.org' } } } })
    expect(w.text()).toContain('example.org')
  })
  it('renders plain text as text, never as html', () => {
    const w = mount(PreviewBody, { props: { preview: { kind: 'text', text: '<b>hi</b>', smart: { type: 'plain' } } } })
    expect(w.find('b').exists()).toBe(false)
    expect(w.text()).toContain('<b>hi</b>')
  })
  it('renders images and files', () => {
    const img = mount(PreviewBody, { props: { preview: { kind: 'image', dataUrl: 'data:image/png;base64,AQID', width: 10, height: 5 } } })
    expect(img.find('img').attributes('src')).toBe('data:image/png;base64,AQID')
    const file = mount(PreviewBody, { props: { preview: { kind: 'file', name: 'report.pdf' } } })
    expect(file.text()).toContain('report.pdf')
  })
})
