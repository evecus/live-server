<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { hexColor, type DanmakuMessage } from '../types'

const props = defineProps<{ platform: string; room: string; enabled: boolean }>()
const emit = defineEmits<{
  (e: 'online', n: number): void
  (e: 'log', text: string): void
}>()

const canvas = ref<HTMLCanvasElement | null>(null)
let ws: WebSocket | null = null
let raf = 0

interface Item { text: string; color: string; x: number; y: number; w: number; speed: number }
let items: Item[] = []

function ctx2d(): CanvasRenderingContext2D | null {
  const c = canvas.value
  if (!c) return null
  const dpr = Math.min(window.devicePixelRatio || 1, 2)
  const w = Math.floor(c.clientWidth * dpr)
  const h = Math.floor(c.clientHeight * dpr)
  if (c.width !== w || c.height !== h) { c.width = w; c.height = h }
  return c.getContext('2d')
}

function push(user: string, msg: string, color?: number) {
  const ctx = ctx2d()
  const c = canvas.value
  if (!ctx || !c || !msg) return
  const text = user ? `${user}：${msg}` : msg
  ctx.font = 'bold 20px sans-serif'
  const y = 16 + (items.length % Math.max(1, Math.floor(c.height / 34))) * 34
  items.push({
    text, color: hexColor(color), x: c.width, y,
    w: ctx.measureText(text).width, speed: 2 + Math.random() * 1.6,
  })
  if (items.length > 120) items.shift()
}

function loop() {
  const ctx = ctx2d()
  const c = canvas.value
  if (ctx && c) {
    ctx.clearRect(0, 0, c.width, c.height)
    ctx.font = 'bold 20px sans-serif'
    ctx.textBaseline = 'top'
    ctx.shadowColor = '#000'
    ctx.shadowBlur = 4
    items = items.filter((it) => {
      it.x -= it.speed
      ctx.fillStyle = it.color
      ctx.fillText(it.text, it.x, it.y)
      return it.x + it.w > 0
    })
    ctx.shadowBlur = 0
  }
  raf = requestAnimationFrame(loop)
}

function disconnect() {
  if (ws) { try { ws.close() } catch { /* ignore */ } ws = null }
}

function connect() {
  disconnect()
  items = []
  if (!props.enabled || !props.platform || !props.room) return
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
      push(m.userName || '', m.message || '', m.color)
      emit('log', `${m.userName || '匿名'}：${m.message || ''}`)
    } else if (m.type === 'online') {
      emit('online', m.online || 0)
    } else {
      emit('log', `[${m.type}] ${m.message || ''}`)
    }
  }
  ws.onclose = () => emit('log', '弹幕连接已断开')
  ws.onerror = () => emit('log', '弹幕连接异常')
}

onMounted(() => { connect(); loop() })
watch(() => [props.platform, props.room, props.enabled], connect)
onBeforeUnmount(() => { disconnect(); cancelAnimationFrame(raf) })
</script>

<template>
  <canvas ref="canvas" class="danmaku-canvas"></canvas>
</template>
