<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '../api'

const router = useRouter()
const password = ref('')
const show = ref(false)
const err = ref('')
const loading = ref(false)

async function submit() {
  err.value = ''
  loading.value = true
  try {
    await api.login(password.value)
    window.dispatchEvent(new CustomEvent('silm-auth-changed'))
    router.replace('/')
  } catch (e) {
    err.value = (e as Error).message
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="login-bg">
    <div class="login-card">
      <div class="logo">
        <span class="mark">
          <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2.5" y="5" width="19" height="14" rx="3" /><path d="M10 9.5l5 2.5-5 2.5z" fill="currentColor" stroke="none" /></svg>
        </span>
        简<span class="b">播</span>
      </div>
      <div class="lock">
        <svg width="23" height="23" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="10" width="16" height="11" rx="2.5" /><path d="M8 10V7a4 4 0 0 1 8 0v3" /><circle cx="12" cy="15.5" r="1.2" fill="currentColor" stroke="none" /></svg>
      </div>
      <h1>登录面板</h1>
      <p class="sub">服务端已开启访问验证，请输入访问密码以继续使用直播聚合面板。</p>
      <div class="field">
        <label>访问密码</label>
        <div class="password-wrap">
          <input class="input" :type="show ? 'text' : 'password'" v-model="password" @keyup.enter="submit"
            placeholder="请输入访问密码" autocomplete="current-password" autofocus />
          <button class="eye" type="button" @click="show = !show" :aria-label="show ? '隐藏密码' : '显示密码'">
            <svg v-if="!show" width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12z" /><circle cx="12" cy="12" r="2.6" /></svg>
            <svg v-else width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M3 3l18 18M10.6 6.1A9.8 9.8 0 0 1 12 6c6.4 0 10 6 10 6a17 17 0 0 1-3 3.6M6.3 7.5A17 17 0 0 0 2 12s3.6 6 10 6a9.7 9.7 0 0 0 3.5-.6" /></svg>
          </button>
        </div>
      </div>
      <div v-if="err" class="error" style="margin-bottom: 12px">{{ err }}</div>
      <button class="btn primary block" style="margin-top: 6px; padding: 11px" :disabled="loading || !password" @click="submit">
        {{ loading ? '登录中…' : '登录面板' }}
      </button>
      <div class="login-foot">简播 · 安全访问 · 会话保存在当前浏览器</div>
    </div>
  </div>
</template>
