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

export interface Settings {
  cookies: { douyin: string; kuaishou: string }
}

export interface DanmakuMessage {
  type: 'chat' | 'online' | 'error' | 'info'
  userName?: string
  message?: string
  color?: number
  online?: number
}

export function proxyImage(url: string): string {
  if (!url) return ''
  return `/api/proxy/image?u=${encodeURIComponent(url)}`
}

export function hexColor(color?: number): string {
  if (!color) return '#ffffff'
  return `#${(color & 0xffffff).toString(16).padStart(6, '0')}`
}
