<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import Hls from 'hls.js'
import mpegts from 'mpegts.js'

const props = defineProps<{ url: string; type: 'hls' | 'flv' }>()
const video = ref<HTMLVideoElement | null>(null)
let player: any = null

function destroy() {
  if (!player) return
  try {
    if (typeof player.destroy === 'function') player.destroy()
    else if (typeof player.unload === 'function') player.unload()
  } catch { /* ignore */ }
  player = null
}

function setup() {
  destroy()
  const el = video.value
  if (!el || !props.url) return
  if (props.type === 'flv') {
    if (mpegts && mpegts.isSupported()) {
      player = mpegts.createPlayer(
        { type: 'flv', isLive: true, url: props.url },
        { enableStashBuffer: false, stashInitialSize: 128, autoCleanupSourceBuffer: true },
      )
      player.attachMediaElement(el)
      player.load()
      try { player.play?.() } catch { /* ignore */ }
    } else {
      el.src = props.url
      el.play().catch(() => {})
    }
  } else {
    if (Hls.isSupported()) {
      player = new Hls({ liveDurationInfinity: true, lowLatencyMode: true, enableWorker: true })
      player.loadSource(props.url)
      player.attachMedia(el)
      player.on(Hls.Events.MANIFEST_PARSED, () => el.play().catch(() => {}))
      player.on(Hls.Events.ERROR, (_e: unknown, data: { fatal?: boolean }) => {
        if (data?.fatal) {
          player?.destroy()
          player = null
        }
      })
    } else if (el.canPlayType('application/vnd.apple.mpegurl')) {
      el.src = props.url
      el.play().catch(() => {})
    }
  }
}

onMounted(setup)
watch(() => [props.url, props.type], setup)
onBeforeUnmount(destroy)
</script>

<template>
  <video ref="video" controls autoplay playsinline></video>
</template>
