<script setup lang="ts">
const props = withDefaults(defineProps<{ name: string; size?: number }>(), { size: 16 })

const paths: Record<string, string> = {
  windows: 'M3 5l7-1v7H3zM11 4l10-1.5V11H11zM3 12h7v7l-7-1zM11 12h10v8.5L11 19z',
  mac: 'M16 3c0 2-1.5 3.5-3 3.5C13 4.5 14.5 3 16 3zM18.5 13c0-2.5 2-3.5 2-3.5-1-1.5-2.7-1.8-3.3-1.8-1.4-.1-2.7.8-3.4.8s-1.8-.8-3-.8C8.3 7.7 6 9.4 6 13c0 2.2.8 4.5 1.9 6 .9 1.3 1.7 2 2.7 2 1 0 1.4-.6 2.6-.6s1.6.6 2.6.6c1.1 0 1.8-1.1 2.6-2.2.5-.7.8-1.4 1-1.8-1.8-.8-2.9-2.3-2.9-4z',
  linux: 'M12 3c-2 0-3 1.8-3 4 0 1.4-1.6 3-2.6 5.2-.9 2-.4 3.8.9 4.6L6 19h12l-1.3-2.2c1.3-.8 1.8-2.6.9-4.6C16.6 10 15 8.4 15 7c0-2.2-1-4-3-4z',
  link: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
  file: 'M6 3h8l5 5v13H6zM14 3v5h5',
  image: 'M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15 9.5a1.5 1.5 0 1 0 0-.01',
  text: 'M5 6h14M5 11h14M5 16h9',
  code: 'M9 7l-5 5 5 5M15 7l5 5-5 5',
  catch: 'M12 4v11M7 11l5 5 5-5M5 20h14',
  close: 'M6 6l12 12M18 6L6 18',
  search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4',
  gear: 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1',
  timeline: 'M4 6h16M4 12h16M4 18h10',
  plus: 'M12 5v14M5 12h14',
  send: 'M4 12l16-8-6 16-2-7z',
  save: 'M5 4h11l3 3v13H5zM8 4v5h7V4M8 20v-6h8v6',
}
</script>

<template>
  <svg
    :width="props.size"
    :height="props.size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path :d="paths[props.name] ?? paths.file" />
  </svg>
</template>
