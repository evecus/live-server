export interface SiteInfo {
  id: string
  name: string
  searchable: boolean
  danmaku: boolean
}

export interface SearchItem {
  roomId: string
  title: string
  cover: string
  userName: string
  avatar: string
  online: number
  liveStatus: boolean
}

export interface RoomDetail {
  platform: string
  roomId: string
  title: string
  cover: string
  userName: string
  userAvatar: string
  online: number
  status: boolean
  url: string
  introduction: string
}

export interface Line {
  name: string
  type: 'hls' | 'flv'
  url: string
}

export interface Quality {
  name: string
  key: string
  lines: Line[]
}

export interface PlayInfo {
  qualities: Quality[]
  danmaku: boolean
  roomId: string
}

export interface Follow {
  id: string
  platform: string
  roomId: string
  title: string
  cover: string
  userName: string
  addTime: number
  top: boolean
  live: boolean
}

/** 面板访问验证：服务端不回显密码，hasPassword 表示是否已设置 */
export interface PanelAuthSettings {
  enabled: boolean
  password: string
  hasPassword?: boolean
}

export type DanmakuColorMode = 'platform' | 'custom' | 'white' | 'rainbow'

/** 弹幕显示设置，服务端持久化，多设备共享 */
export interface DanmakuSettings {
  enabled: boolean
  fontSize: number
  area: number
  lineGap: number
  itemGap: number
  duration: number
  colorMode: DanmakuColorMode
  color: string
  opacity: number
  bold: boolean
  stroke: boolean
  showUser: boolean
}

export interface Settings {
  cookies: { douyin: string; kuaishou: string }
  panelAuth: PanelAuthSettings
  danmaku: DanmakuSettings
}

export interface DanmakuMessage {
  type: 'chat' | 'online' | 'error' | 'info'
  userName?: string
  message?: string
  color?: number
  online?: number
}

export const defaultDanmaku = (): DanmakuSettings => ({
  enabled: true,
  fontSize: 22,
  area: 60,
  lineGap: 10,
  itemGap: 64,
  duration: 9,
  colorMode: 'platform',
  color: '#ffffff',
  opacity: 1,
  bold: true,
  stroke: true,
  showUser: true,
})

export const defaultSettings = (): Settings => ({
  cookies: { douyin: '', kuaishou: '' },
  panelAuth: { enabled: false, password: '', hasPassword: false },
  danmaku: defaultDanmaku(),
})

/** 兼容旧数据 / 缺字段：用默认值补齐，避免界面出现 NaN */
export function normalizeSettings(raw: Partial<Settings> | null | undefined): Settings {
  const d = defaultSettings()
  if (!raw) return d
  return {
    cookies: {
      douyin: raw.cookies?.douyin ?? '',
      kuaishou: raw.cookies?.kuaishou ?? '',
    },
    panelAuth: {
      enabled: !!raw.panelAuth?.enabled,
      password: raw.panelAuth?.password ?? '',
      hasPassword: !!raw.panelAuth?.hasPassword,
    },
    danmaku: { ...d.danmaku, ...(raw.danmaku || {}) },
  }
}

export function proxyImage(url: string): string {
  if (!url) return ''
  if (url.startsWith('data:')) return url
  return `/api/proxy/image?u=${encodeURIComponent(url)}`
}

export function hexColor(color?: number): string {
  if (!color) return '#ffffff'
  return `#${(color & 0xffffff).toString(16).padStart(6, '0')}`
}
