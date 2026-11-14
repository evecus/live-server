/**
 * 静态预览模式：仅在 URL 带 ?demo=1（或构建时 VITE_DEMO=1）时启用。
 * 用本地数据模拟后端，方便脱离 Rust 服务直接查看界面效果；正式部署不受影响。
 */
import { defaultSettings, normalizeSettings, type Follow, type Settings } from './types'

const K_SETTINGS = 'silm_demo_settings'
const K_AUTH = 'silm_demo_auth'
const K_LOGGED = 'silm_demo_logged'
const K_FOLLOWS = 'silm_demo_follows'

export function isDemo(): boolean {
  if (import.meta.env.VITE_DEMO === '1') return true
  try { return new URLSearchParams(location.search).get('demo') === '1' } catch { return false }
}

function read<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    return raw ? (JSON.parse(raw) as T) : fallback
  } catch { return fallback }
}
function write(key: string, v: unknown) {
  try { localStorage.setItem(key, JSON.stringify(v)) } catch { /* ignore */ }
}
const wait = (ms = 110) => new Promise((r) => setTimeout(r, ms))

function cover(text: string, a: string, b: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><circle cx="520" cy="70" r="170" fill="#ffffff" opacity=".10"/><circle cx="90" cy="330" r="130" fill="#ffffff" opacity=".08"/><text x="50%" y="52%" text-anchor="middle" fill="#ffffff" font-size="62" font-weight="700" font-family="sans-serif" opacity=".92">${text}</text></svg>`
  return `data:image/svg+xml,${encodeURIComponent(svg)}`
}

function seedFollows(): Follow[] {
  return [
    { id: 'bilibili:21452505', platform: 'bilibili', roomId: '21452505', title: '2026 王者荣耀 KPL 春季赛 · 常规赛', cover: cover('KPL', '#b8860b', '#f0c05a'), userName: '王者荣耀赛事', addTime: 1710000000, top: true, live: true },
    { id: 'douyu:9999', platform: 'douyu', roomId: '9999', title: '两万3战力貂蝉，从零单排上荣耀', cover: cover('貂蝉', '#4caf50', '#a8e063'), userName: '王者荣耀张三', addTime: 1709000000, top: true, live: false },
    { id: 'huya:660000', platform: 'huya', roomId: '660000', title: '打工人的深夜放松局', cover: cover('小油条', '#7b61ff', '#c4a7ff'), userName: '打工小油条', addTime: 1708000000, top: false, live: false },
    { id: 'douyin:123456', platform: 'douyin', roomId: '123456', title: '迷茫小树叶 · 随手聊聊', cover: '', userName: '迷茫小树叶', addTime: 1707000000, top: false, live: false },
    { id: 'kuaishou:8888', platform: 'kuaishou', roomId: '8888', title: '今晚冲国服韩信，来就带飞', cover: cover('韩信', '#ff6b6b', '#ffa36b'), userName: '国服韩信本人', addTime: 1706000000, top: false, live: true },
    { id: 'bilibili:6', platform: 'bilibili', roomId: '6', title: '深夜电台 · 一起听歌聊天', cover: cover('电台', '#2b6cb0', '#38b2ac'), userName: '晚安电台', addTime: 1705000000, top: false, live: true },
    { id: 'douyin:7788', platform: 'douyin', roomId: '7788', title: '深渊满星教学，萌新也能看懂', cover: '', userName: '原神小课堂', addTime: 1704000000, top: false, live: false },
    { id: 'xhs:5566', platform: 'xhs', roomId: '5566', title: '周末户外野钓，风景比鱼好看', cover: '', userName: '老张爱钓鱼', addTime: 1703000000, top: false, live: false },
  ]
}

const SITES = [
  { id: 'bilibili', name: '哔哩哔哩', searchable: true, danmaku: true },
  { id: 'douyu', name: '斗鱼', searchable: true, danmaku: true },
  { id: 'huya', name: '虎牙', searchable: true, danmaku: true },
  { id: 'douyin', name: '抖音', searchable: true, danmaku: true },
  { id: 'kuaishou', name: '快手', searchable: true, danmaku: true },
  { id: 'xhs', name: '小红书', searchable: false, danmaku: false },
]

const SEARCH = [
  { roomId: '21452505', title: 'KPL 春季赛 官方直播间', cover: cover('KPL', '#b8860b', '#f0c05a'), userName: '王者荣耀赛事', avatar: '', online: 128340, liveStatus: true },
  { roomId: '545068', title: '一起看赛季更新公告', cover: cover('更新', '#5b6cff', '#22c1c3'), userName: '解说瓶子', avatar: '', online: 23410, liveStatus: true },
  { roomId: '9999', title: '貂蝉专场 · 冲国服', cover: '', userName: '王者荣耀张三', avatar: '', online: 0, liveStatus: false },
  { roomId: '31415', title: '娱乐五排，快乐为主', cover: cover('五排', '#ff6b9d', '#ffb37b'), userName: '峡谷喜剧人', avatar: '', online: 8720, liveStatus: true },
]

function authState(): { enabled: boolean; password: string } {
  return read(K_AUTH, { enabled: false, password: '' })
}
function logged(): boolean { return read<boolean>(K_LOGGED, false) === true }
function follows(): Follow[] {
  const saved = read<Follow[] | null>(K_FOLLOWS, null)
  if (saved) return saved
  const seed = seedFollows()
  write(K_FOLLOWS, seed)
  return seed
}
function settings(): Settings {
  return normalizeSettings(read<Partial<Settings> | null>(K_SETTINGS, null))
}

function unauthorized(): never {
  window.dispatchEvent(new CustomEvent('silm-unauthorized'))
  throw new Error('未登录或登录已失效')
}

export async function mockRequest<T>(path: string, init?: RequestInit): Promise<T> {
  await wait()
  const method = (init?.method || 'GET').toUpperCase()
  const url = new URL(path, location.origin)
  const p = url.pathname
  const body = init?.body ? JSON.parse(String(init.body)) as Record<string, unknown> : {}
  const auth = authState()

  const isPublic = p === '/api/auth/status' || p === '/api/auth/login' || p === '/api/auth/logout' || p === '/health'
  if (!isPublic && auth.enabled && !logged()) unauthorized()

  const out = (v: unknown) => v as T

  if (p === '/api/auth/status') return out({ enabled: auth.enabled, authenticated: !auth.enabled || logged() })
  if (p === '/api/auth/login') {
    if (!auth.enabled) return out({ ok: true, enabled: false })
    if (body.password !== auth.password) { throw new Error('密码错误') }
    write(K_LOGGED, true)
    return out({ ok: true, enabled: true })
  }
  if (p === '/api/auth/logout') { write(K_LOGGED, false); return out({ ok: true }) }

  if (p === '/api/settings') {
    if (method === 'GET') {
      const s = settings()
      s.panelAuth.enabled = auth.enabled
      s.panelAuth.hasPassword = !!auth.password
      s.panelAuth.password = ''
      return out(s)
    }
    const s = normalizeSettings(body as Partial<Settings>)
    let password = auth.password
    if (s.panelAuth.password.trim()) password = s.panelAuth.password.trim()
    if (s.panelAuth.enabled && !password) throw new Error('开启访问验证前请先设置访问密码')
    write(K_AUTH, { enabled: s.panelAuth.enabled, password })
    s.panelAuth.password = ''
    s.panelAuth.hasPassword = !!password
    write(K_SETTINGS, s)
    return out({ ok: true })
  }

  if (p === '/api/sites') return out({ sites: SITES })
  if (p === '/api/follows' && method === 'GET') return out({ items: follows() })
  if (p === '/api/follows' && method === 'POST') {
    const list = follows()
    const f = body as Partial<Follow>
    const id = `${f.platform}:${f.roomId}`
    const old = list.find((x) => x.id === id)
    if (old) {
      Object.assign(old, { title: f.title || old.title, cover: f.cover || old.cover, userName: f.userName || old.userName })
      write(K_FOLLOWS, list)
      return out({ item: old })
    }
    const item: Follow = { id, platform: String(f.platform || ''), roomId: String(f.roomId || ''), title: String(f.title || ''), cover: String(f.cover || ''), userName: String(f.userName || ''), addTime: Date.now() / 1000, top: false, live: false }
    list.unshift(item)
    write(K_FOLLOWS, list)
    return out({ item })
  }
  if (p === '/api/follows/export') return out({ app: 'silm-live', version: 1, follows: follows() })
  if (p === '/api/follows/import') return out({ imported: 0 })
  if (p.startsWith('/api/follows/')) {
    const id = decodeURIComponent(p.slice('/api/follows/'.length))
    const list = follows()
    if (method === 'DELETE') { write(K_FOLLOWS, list.filter((x) => x.id !== id)); return out({ ok: true }) }
    if (method === 'PATCH') { const f = list.find((x) => x.id === id); if (f) f.top = !!body.top; write(K_FOLLOWS, list); return out({ ok: true }) }
  }

  if (p === '/api/search') return out({ hasMore: false, items: SEARCH })
  if (p === '/api/parse') {
    const text = String(body.text || '')
    const m = text.match(/(\d{4,})/)
    let platform = 'bilibili'
    if (text.includes('douyu')) platform = 'douyu'
    else if (text.includes('huya')) platform = 'huya'
    else if (text.includes('douyin')) platform = 'douyin'
    else if (text.includes('kuaishou')) platform = 'kuaishou'
    return out({ platform, roomId: m ? m[1] : '21452505' })
  }

  const roomMatch = p.match(/^\/api\/rooms\/([^/]+)\/([^/]+)(\/play|\/status)?$/)
  if (roomMatch) {
    const [, platform, room, suffix] = roomMatch
    const f = follows().find((x) => x.roomId === room)
    if (suffix === '/status') return out({ live: f?.live ?? false })
    if (suffix === '/play') return out({ qualities: [], danmaku: true, roomId: room })
    return out({
      detail: {
        platform, roomId: room,
        title: f?.title || '', cover: f?.cover || '', userName: f?.userName || '主播',
        userAvatar: '', online: 12580, status: f?.live ?? false, url: '',
        introduction: '演示数据 · 静态预览模式',
      },
    })
  }

  return out({ ok: true })
}
