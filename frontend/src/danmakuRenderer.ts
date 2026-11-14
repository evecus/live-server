import type { DanmakuSettings } from './types'

interface Item {
  text: string
  color: string
  x: number
  y: number
  w: number
  speed: number
  track: number
}
interface Pending { text: string; color: string }

const PALETTE = ['#ffffff', '#ffd166', '#7ae7ff', '#ff9ec4', '#a8ffb2', '#c4b5ff', '#ffb37b']
const FONT = "'PingFang SC', 'Microsoft YaHei', 'Segoe UI', sans-serif"

/** Canvas 弹幕渲染：轨道分配、速度/间距/区域/颜色/描边等全部由设置驱动 */
export class DanmakuRenderer {
  private ctx: CanvasRenderingContext2D | null
  private settings: DanmakuSettings
  private items: Item[] = []
  private pending: Pending[] = []
  private width = 0
  private height = 0
  private dpr = 1
  private raf = 0
  private last = 0
  private running = false

  constructor(private canvas: HTMLCanvasElement, settings: DanmakuSettings) {
    this.ctx = canvas.getContext('2d')
    this.settings = { ...settings }
    this.syncSize()
  }

  setSettings(s: DanmakuSettings) {
    const fontChanged = s.fontSize !== this.settings.fontSize || s.bold !== this.settings.bold
    this.settings = { ...s }
    if (fontChanged) {
      this.items = []
      this.pending = []
    }
    this.syncSize()
  }

  clear() {
    this.items = []
    this.pending = []
    if (this.ctx) this.ctx.clearRect(0, 0, this.width, this.height)
  }

  /** 预览用：把已有弹幕分散到画面内，避免打开页面时要等弹幕从右侧飘入 */
  scatter() {
    for (const it of this.items) {
      const span = Math.max(1, this.width - it.w - 8 * this.dpr)
      it.x = span * (0.08 + Math.random() * 0.88)
    }
    this.draw()
  }

  private syncSize() {
    this.dpr = Math.min(window.devicePixelRatio || 1, 2)
    const w = Math.max(1, Math.floor(this.canvas.clientWidth * this.dpr))
    const h = Math.max(1, Math.floor(this.canvas.clientHeight * this.dpr))
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w
      this.canvas.height = h
    }
    this.width = w
    this.height = h
  }

  private font(): string {
    const s = this.settings
    return `${s.bold ? 700 : 500} ${Math.max(8, s.fontSize) * this.dpr}px ${FONT}`
  }

  private lineHeight(): number {
    return (Math.max(8, this.settings.fontSize) + Math.max(0, this.settings.lineGap)) * this.dpr
  }

  private rows(): number {
    const top = 8 * this.dpr
    const usable = Math.max(this.lineHeight(), this.height * (this.settings.area / 100) - top)
    return Math.max(1, Math.floor(usable / this.lineHeight()))
  }

  private resolveColor(platformColor: string): string {
    switch (this.settings.colorMode) {
      case 'custom': return this.settings.color || '#ffffff'
      case 'white': return '#ffffff'
      case 'rainbow': return PALETTE[Math.floor(Math.random() * PALETTE.length)]
      default: return platformColor || '#ffffff'
    }
  }

  /** 某轨道上最后一条弹幕尾部的当前位置；空轨道为 -Infinity */
  private trackRight(track: number): number {
    let right = Number.NEGATIVE_INFINITY
    for (const it of this.items) {
      if (it.track === track) right = Math.max(right, it.x + it.w)
    }
    return right
  }

  push(text: string, platformColor = '#ffffff') {
    if (!text.trim() || !this.settings.enabled) return
    this.pending.push({ text, color: this.resolveColor(platformColor) })
    if (this.pending.length > 40) this.pending.shift()
    this.flushPending()
  }

  private flushPending() {
    const ctx = this.ctx
    if (!ctx || !this.pending.length) return
    ctx.font = this.font()
    const gap = Math.max(0, this.settings.itemGap) * this.dpr
    const rows = this.rows()
    const lineH = this.lineHeight()
    const top = 8 * this.dpr
    const duration = Math.max(1.5, this.settings.duration)

    const rest: Pending[] = []
    for (const p of this.pending) {
      const w = ctx.measureText(p.text).width + 6 * this.dpr
      let best = -1
      let bestRight = Number.POSITIVE_INFINITY
      for (let t = 0; t < rows; t++) {
        const r = this.trackRight(t)
        if (r <= this.width - gap && r < bestRight) { best = t; bestRight = r }
      }
      if (best < 0) { rest.push(p); continue }
      this.items.push({
        text: p.text,
        color: p.color,
        x: this.width,
        y: top + best * lineH,
        w,
        speed: (this.width + w) / duration,
        track: best,
      })
    }
    this.pending = rest
    if (this.items.length > 160) this.items.splice(0, this.items.length - 160)
  }

  private draw() {
    const ctx = this.ctx
    if (!ctx) return
    ctx.clearRect(0, 0, this.width, this.height)
    ctx.font = this.font()
    ctx.textBaseline = 'top'
    ctx.lineJoin = 'round'
    ctx.lineWidth = 3 * this.dpr
    ctx.strokeStyle = 'rgba(8, 12, 28, .72)'
    ctx.globalAlpha = Math.min(1, Math.max(0.15, this.settings.opacity))
    const stroke = this.settings.stroke
    for (const it of this.items) {
      if (stroke) ctx.strokeText(it.text, it.x, it.y)
      ctx.fillStyle = it.color
      ctx.fillText(it.text, it.x, it.y)
    }
    ctx.globalAlpha = 1
  }

  private frame = (now: number) => {
    if (!this.running) return
    const dt = this.last ? Math.min(0.05, (now - this.last) / 1000) : 0.016
    this.last = now
    this.syncSize()
    this.items = this.items.filter((it) => {
      it.x -= it.speed * dt
      return it.x + it.w > 0
    })
    if (this.pending.length) this.flushPending()
    this.draw()
    this.raf = requestAnimationFrame(this.frame)
  }

  start() {
    if (this.running) return
    this.running = true
    this.last = 0
    this.raf = requestAnimationFrame(this.frame)
  }

  stop() {
    this.running = false
    cancelAnimationFrame(this.raf)
  }
}

/** 设置页预览用的示例弹幕 */
export const SAMPLE_DANMAKU: Array<{ user: string; text: string; color: string }> = [
  { user: '小橘子', text: '这波操作太秀了！', color: '#ffd166' },
  { user: '夜航星', text: '主播今天状态真好', color: '#7ae7ff' },
  { user: '阿白', text: '前方高能预警', color: '#ffffff' },
  { user: '路过的猫', text: '哈哈哈哈哈哈哈', color: '#ff9ec4' },
  { user: '一只柚子', text: '666666666', color: '#a8ffb2' },
  { user: '晚风', text: '已经三连关注啦', color: '#c4b5ff' },
  { user: '小K', text: '弹幕效果实时预览 ✨', color: '#ffffff' },
  { user: '柠檬茶', text: '这个设置也太好用了', color: '#ffb37b' },
]
