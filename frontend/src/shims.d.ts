declare module 'mpegts.js' {
  const mpegts: any
  export default mpegts
}

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}
