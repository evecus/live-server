# 简播 · silm-live-server

把上传的 Android 版「简播」多平台直播客户端改造成 **Linux 常驻服务 + 网页面板**：

- **后端**：Rust（axum + tokio），跨平台纯 Rust 技术栈，运行时**不依赖 Node.js / JVM / nginx**
- **前端**：Vue 3 + Vite，构建产物是静态文件，编译进二进制后可单文件交付
- **流代理**：所有 FLV / HLS 流由后端转发并注入 Referer / UA / Cookie，解决浏览器跨域与防盗链
- **鉴权**：设置页可随时开关 / 改密码并立即生效，也可由配置项或命令行预设，默认关闭
- **弹幕**：字号、屏占比、行间距、水平间距、颜色、速度、透明度、加粗、描边、用户名均可调，设置页带实时预览
- **界面**：白色面板 + 现代灵动主题，关注卡片不再显示「未命名直播间」占位文字
- **平台**：哔哩哔哩 / 斗鱼 / 虎牙 / 抖音 / 快手 / 小红书，一次到位

## 架构

```
浏览器（Vue3 面板：关注 / 搜索 / 播放 / 设置）
      │  REST + WebSocket(弹幕)
      ▼
Rust 服务 axum + tokio  ── 内嵌静态页面(rust-embed)
      ├─ /api/rooms/*     6 个平台站点实现（房间 / 清晰度 / 线路 / 搜索）
      ├─ /api/proxy/*     流代理 + HLS m3u8 重写 + 封面代理
      ├─ /ws/danmaku      弹幕长连接，同一房间多页面复用一条上游连接
      ├─ sign/            rquickjs 内嵌 QuickJS，执行 3 个 JS 签名脚本
      └─ store/           JSON 文件持久化关注 / Cookie / 设置
      ▼
各平台 API / 弹幕 WebSocket / 直播 CDN
```

> 抖音 `a_bogus` / `MSSDK`、斗鱼 `crypto-js` 三份 JS 资产原样复用（Android 版本来就用 QuickJS 执行），
> Rust 侧用 `rquickjs` 在专用线程内嵌执行，**没有 Node 子进程**，脚本通过 `include_str!` 编进二进制。

## 目录

```
Cargo.toml              后端依赖
config.example.toml     配置样例（复制为 config.toml）
assets/                 3 个平台签名脚本（编译期内嵌）
src/
  main.rs               入口
  api.rs                 REST / WS / 流代理 / 静态托管
  auth.rs                可选鉴权（支持设置页运行时开关）
  config.rs  state.rs  store.rs  http_client.rs  error.rs  models.rs
  sign.rs                QuickJS 签名引擎
  sites/                 6 个平台站点实现 + 链接解析
  danmaku/               6 个平台弹幕协议 + 广播 Hub
  pb.rs  tars.rs  util.rs  identity.rs
frontend/                Vue 3 + Vite 源码
  src/danmakuRenderer.ts  弹幕 Canvas 渲染器（轨道分配 / 样式，播放页与设置页预览共用）
  src/mock.ts             ?demo=1 静态预览数据（仅用于无后端时看界面）
frontend/dist/           构建好的静态页面（内嵌进二进制）
```

## 快速开始

项目已附带构建好的 `frontend/dist/`，编译时由 `rust-embed` 直接内嵌进二进制，
**部署全过程不需要 Node.js**，只需要 Rust 工具链：

```bash
# 1. 编译后端（无需先执行 pnpm install / pnpm build）
cargo build --release

# 2. 配置（可选，默认鉴权关闭、监听 0.0.0.0:8080）
cp config.example.toml config.toml

# 3. 运行
./target/release/silm-live-server            # 默认 0.0.0.0:8080
```

浏览器打开 `http://<服务器IP>:8080`。

> **只看界面效果**：在地址后加 `?demo=1`（例如 `http://localhost:8080/?demo=1#/settings`）即可进入静态预览模式，
> 用内置示例数据展示关注、搜索、播放、弹幕预览与访问验证流程，不改动服务端数据。该参数仅在显式添加时生效。

**只有修改前端源码时**才需要 Node（此时装不装 Node 与运行时无关，仅构建期使用）：

```bash
cd frontend && pnpm install && pnpm build && cd ..
cargo build --release    # 重新编译，让新的 dist/ 内嵌进二进制
```

开发调试可前后端分离：`frontend/` 下 `pnpm dev`（Vite 已配置 `/api`、`/ws` 代理到 8080），
另开一个终端跑 `cargo run`。

命令行覆盖（也可用同名环境变量）：

```bash
./silm-live-server --listen 0.0.0.0:9000 --data-dir /var/lib/silm \
  --auth-enabled true --auth-password '你的密码'
```

环境变量：`SILM_LISTEN` / `SILM_CONFIG` / `SILM_DATA_DIR` / `SILM_AUTH_ENABLED` /
`SILM_AUTH_PASSWORD` / `SILM_UPSTREAM_PROXY`。

## 鉴权

默认关闭，局域网直接访问。**推荐直接在「设置 → 面板访问验证」中开关**：改动立即生效并持久化到
`data/silm.json`，重启服务后保持；密码不会回显到页面（`GET /api/settings` 只返回 `hasPassword`），
密码框留空表示沿用已设密码。

配置文件或命令行可作为初始状态（面板尚未管理过时生效）：

```toml
[auth]
enabled = true
password = "一个足够长的密码"
```

- 开启后前端自动跳登录页，`POST /api/auth/login` 校验密码后下发 HttpOnly Cookie
- 所有 `/api/*` 与 `/ws/*` 校验 Cookie 或 `Authorization: Bearer <token>`，也支持 `?token=`（便于 WS / 流地址）
- 静态页面本身放行，API 返回 401 时前端自动跳登录；`POST /api/auth/logout` 清除会话 Cookie
- 会话令牌由「启动随机密钥 + 密码」派生，改密码后旧会话立即失效，重启服务后需重新登录
- 开启时必须设置密码；在面板填写新密码保存后会自动完成一次登录，不会把自己锁在门外

## 弹幕设置

「设置 → 弹幕显示」的参数保存在服务端（`GET/POST /api/settings` 的 `danmaku` 字段），多设备共享；
设置页带实时预览，保存后播放页通过 `silm-settings-changed` 事件即时应用。

| 参数 | 说明 | 默认值 |
|---|---|---|
| `enabled` | 是否启用弹幕渲染 | `true` |
| `fontSize` | 字号 px | `22` |
| `area` | 显示区域占画面高度百分比（屏占比） | `60` |
| `lineGap` | 行间距 px | `10` |
| `itemGap` | 同轨道相邻弹幕最小水平间距 px | `64` |
| `duration` | 横穿一屏所需秒数（越小越快） | `9` |
| `colorMode` | `platform` / `custom` / `white` / `rainbow` | `platform` |
| `color` | 自定义颜色（`custom` 模式生效） | `#ffffff` |
| `opacity` | 文字不透明度 | `1` |
| `bold` / `stroke` / `showUser` | 加粗 / 描边阴影 / 显示用户名前缀 | 均开启 |

渲染器按屏占比与行间距自动计算轨道，优先选择最空闲的一条，拥挤时排队，避免弹幕互相重叠。

## 平台支持

| 平台 | 房间详情 | 播放地址 | 弹幕 | 搜索 | 备注 |
|---|---|---|---|---|---|
| 哔哩哔哩 | ✅ | ✅ FLV/HLS | ✅ brotli 二进制 | ✅ | WBI 签名、游客弹幕 |
| 斗鱼 | ✅ | ✅ 多线路 FLV | ✅ STT | ✅ | QuickJS 执行 `homeH5Enc` 签名脚本 |
| 虎牙 | ✅ | ✅ HLS/FLV | ✅ TARS | ✅ | anticode 签名、CDN 校验 UA |
| 抖音 | ✅ | ✅ FLV/HLS | ✅ protobuf | ✅ 需 Cookie | a_bogus + MSSDK 签名、IM 预取 |
| 快手 | ✅ | ✅ FLV | ✅ protobuf | ✅ 需 Cookie | 页面 `__INITIAL_STATE__` |
| 小红书 | ✅ | ✅ HLS/FLV | ❌ | ❌ | 移动端 UA 的 SSR 数据，仅支持分享链接 |

> 小红书搜索需 x-s 签名、弹幕协议未公开，源码中即标注不支持，本项目保持一致。
> 抖音 / 快手搜索与弹幕凭证建议先在「设置 → 账号 Cookie」中粘贴登录态。

## 主要 API

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/sites` | 平台列表与支持能力 |
| POST | `/api/parse` | 解析分享链接 / 短链 → 平台 + 房间号 |
| GET | `/api/search?platform=&kw=&page=&anchor=` | 搜索直播间 / 主播 |
| GET | `/api/rooms/:platform/:room` | 房间详情 |
| GET | `/api/rooms/:platform/:room/play` | 清晰度 + 线路（已代理化的 URL） |
| GET | `/api/rooms/:platform/:room/status` | 开播状态 |
| GET/POST | `/api/follows` | 关注列表 / 新增 |
| DELETE/PATCH | `/api/follows/:id` | 删除 / 置顶 |
| GET/POST | `/api/follows/export`、`/api/follows/import` | JSON 导入导出（兼容安卓端字段） |
| GET/POST | `/api/settings` | Cookie / 面板访问验证 / 弹幕显示设置（密码不回显） |
| POST | `/api/auth/logout` | 清除会话 Cookie |
| GET | `/api/proxy/stream?u=...` | FLV / 分片流式转发 |
| GET | `/api/proxy/hls?u=...` | m3u8 重写（分片、密钥全部走代理） |
| GET | `/api/proxy/image?u=...` | 封面图片代理 |
| WS | `/ws/danmaku?platform=&room=` | 统一弹幕推送 |

弹幕消息格式：`{"type":"chat"|"online"|"error"|"info","userName":"","message":"","color":16777215,"online":0}`。

## 实现要点

- **不依赖 Node.js**：签名脚本用 Rust 内嵌 QuickJS（`rquickjs`）执行；前端构建产物为静态文件，
  运行时由 `rust-embed` 直接从二进制提供，服务器上无 Node 进程。
- **流代理**：`reqwest::Response::bytes_stream()` → `axum::body::Body::from_stream()`，边拉边转不落地；
  HLS 逐行改写 m3u8 里的 URI（含 `EXT-X-KEY`），并把 referer/ua/cookie/token 透传到每个分片。
- **弹幕广播**：`DanmakuHub` 按 `平台:房间` 维护一条上游 WebSocket，多个网页订阅同一 broadcast channel；
  最后一个订阅者离开时自动断开上游，避免放大连接。
- **跨平台**：除 QuickJS（内嵌 C 源码，由 `cc` 编译）外全部为纯 Rust crate，TLS 使用 rustls，
  无 OpenSSL / 系统库依赖；`cargo build` 在 Linux / macOS / Windows 上均可直接构建。
- **存储**：`data/silm.json`，原子写（tmp + rename），无需数据库。

## systemd 示例

```ini
[Unit]
Description=简播直播聚合服务
After=network-online.target

[Service]
Type=simple
WorkingDirectory=/opt/silm-live-server
ExecStart=/opt/silm-live-server/silm-live-server --listen 0.0.0.0:8080
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

## 免责声明

本项目不存储任何平台数据，接口逻辑移植自开源项目 `dart_simple_live` 与上传的 Android 版简播，
仅供个人学习研究，请勿用于商业用途。各平台接口可能随时变更，出现失效时以实测调整对应 `src/sites/` 实现为准。
