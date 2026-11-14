<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { api } from '../api'
import { isDemo } from '../mock'
import { DanmakuRenderer, SAMPLE_DANMAKU } from '../danmakuRenderer'
import { defaultDanmaku, hexColor, type DanmakuMessage, type DanmakuSettings } from '../types'

const props = defineProps<{ platform: string; room: string; enabled: boolean }>()
const emit = defineEmits<{
  (e: 'online', n: number): void
  (e: 'log', text: string): void
}>()

const canvas = ref<HTMLCanvasElement | null>(null)
const settings = ref<DanmakuSettings>(defaultDanmaku())
let renderer: DanmakuRenderer | null = null
let ws: WebSocket | null = null
let demoTimer = 0
let demoIndex = 0

async function loadSettings() {
  try {
    const s = await api.getSettings()
    settings.value = s.danmaku
    renderer?.setSettings(s.danmaku)
  } catch { /* 保留默认设置 */ }
}

function disconnect() {
  clearInterval(demoTimer)
  demoTimer = 0
  if (ws) { try { ws.close() } catch { /* ignore */ } ws = null }
}

/** 静态预览模式没有后端 WS，用示例弹幕演示同样的渲染效果 */
function startDemo() {
  clearInterval(demoTimer)
  demoIndex = 0
  const feed = () => {
    const item = SAMPLE_DANMAKU[demoIndex++ % SAMPLE_DANMAKU.length]
    const text = settings.value.showUser && item.user ? `${item.user}：${item.text}` : item.text
    renderer?.push(text, item.color)
    emit('log', `${item.user}：${item.text}`)
  }
  feed(); feed()
  demoTimer = window.setInterval(feed, 1100)
}

function connect() {
  disconnect()
  renderer?.clear()
  if (!props.enabled || !settings.value.enabled || !props.platform || !props.room) return
  if (isDemo()) { startDemo(); return }
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const url = `${proto}://${location.host}/ws/danmaku?platform=${encodeURIComponent(props.platform)}&room=${encodeURIComponent(props.room)}`
  try {
    ws = new WebSocket(url)
  } catch (e) {
    emit('log', `弹幕连接失败：${e}`)
    return
  }
  ws.onmessage = (ev) => {
    let m: DanmakuMessage
    try { m = JSON.parse(ev.data) } catch { return }
    if (m.type === 'chat') {
      const user = m.userName || ''
      const msg = m.message || ''
      const text = settings.value.showUser && user ? `${user}：${msg}` : msg
      renderer?.push(text, hexColor(m.color))
      emit('log', `${user || '匿名'}：${msg}`)
    } else if (m.type === 'online') {
      emit('online', m.online || 0)
    } else {
      emit('log', `[${m.type}] ${m.message || ''}`)
    }
  }
  ws.onclose = () => emit('log', '弹幕连接已断开')
  ws.onerror = () => emit('log', '弹幕连接异常')
}

function onSettingsChanged() {
  const before = settings.value.enabled
  loadSettings().then(() => {
    const after = settings.value.enabled
    // 只有开关状态变化（或预览模式需要重置示例流）才重连，字号/颜色等由渲染器即时更新
    if (before !== after || (isDemo() && after)) connect()
    else if (!after) disconnect()
  })
}
function onStorage(e: StorageEvent) {
  if (e.key && e.key.startsWith('silm_demo')) onSettingsChanged()
}

onMounted(async () => {
  if (canvas.value) {
    renderer = new DanmakuRenderer(canvas.value, settings.value)
    renderer.start()
  }
  window.addEventListener('silm-settings-changed', onSettingsChanged)
  window.addEventListener('storage', onStorage)
  await loadSettings()
  connect()
})

watch(() => [props.platform, props.room, props.enabled], connect)
onBeforeUnmount(() => {
  window.removeEventListener('silm-settings-changed', onSettingsChanged)
  window.removeEventListener('storage', onStorage)
  disconnect()
  clearInterval(demoTimer)
  renderer?.stop()
  renderer = null
})
</script>

<template>
  <canvas ref="canvas" class="danmaku-canvas"></canvas>
</template>
