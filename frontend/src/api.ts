import type { Follow, PlayInfo, Quality, RoomDetail, SearchItem, Settings, SiteInfo } from './types'

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const resp = await fetch(path, {
    credentials: 'same-origin',
    ...init,
    headers: { 'Content-Type': 'application/json', ...(init?.headers || {}) },
  })
  if (resp.status === 401) {
    window.dispatchEvent(new CustomEvent('silm-unauthorized'))
    throw new Error('未登录或登录已失效')
  }
  if (!resp.ok) {
    // axum 的参数提取失败会返回纯文本（如 422），不能假设一定是 JSON
    const text = await resp.text().catch(() => '')
    let message = ''
    try {
      message = (JSON.parse(text) as { error?: string }).error || ''
    } catch {
      // 非 JSON 错误体：不把英文技术细节直接抛给用户
    }
    throw new Error(message || `请求失败 (HTTP ${resp.status})`)
  }
  return (await resp.json()) as T
}

function post<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, { method: 'POST', body: JSON.stringify(body) })
}

export const api = {
  authStatus: () => request<{ enabled: boolean; authenticated: boolean }>('/api/auth/status'),
  login: (password: string) => post<{ ok: boolean }>('/api/auth/login', { password }),

  sites: () => request<{ sites: SiteInfo[] }>('/api/sites'),
  parse: (text: string) => post<{ platform: string; roomId: string }>('/api/parse', { text }),

  search: (platform: string, kw: string, page = 1, anchor = false) =>
    request<{ hasMore: boolean; items: SearchItem[] }>(
      `/api/search?platform=${encodeURIComponent(platform)}&kw=${encodeURIComponent(kw)}&page=${page}&anchor=${anchor}`,
    ),

  room: (platform: string, room: string) =>
    request<{ detail: RoomDetail }>(`/api/rooms/${encodeURIComponent(platform)}/${encodeURIComponent(room)}`),
  play: (platform: string, room: string) =>
    request<PlayInfo>(`/api/rooms/${encodeURIComponent(platform)}/${encodeURIComponent(room)}/play`),
  status: (platform: string, room: string) =>
    request<{ live: boolean }>(`/api/rooms/${encodeURIComponent(platform)}/${encodeURIComponent(room)}/status`),

  follows: () => request<{ items: Follow[] }>('/api/follows'),
  addFollow: (f: { platform: string; roomId: string; title?: string; cover?: string; userName?: string }) =>
    post<{ item: Follow }>('/api/follows', f),
  delFollow: (id: string) => request<{ ok: boolean }>(`/api/follows/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  topFollow: (id: string, top: boolean) =>
    request<{ ok: boolean }>(`/api/follows/${encodeURIComponent(id)}`, { method: 'PATCH', body: JSON.stringify({ top }) }),

  exportFollows: () => request<unknown>('/api/follows/export'),
  importFollows: (data: unknown) => post<{ imported: number }>('/api/follows/import', data),

  getSettings: () => request<Settings>('/api/settings'),
  setSettings: (s: Settings) => post<{ ok: boolean }>('/api/settings', s),
}

export type { Quality }
