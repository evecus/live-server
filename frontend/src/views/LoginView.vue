<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '../api'

const router = useRouter()
const password = ref('')
const err = ref('')
const loading = ref(false)

async function submit() {
  err.value = ''
  loading.value = true
  try {
    await api.login(password.value)
    router.replace('/')
  } catch (e) {
    err.value = (e as Error).message
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="page" style="max-width: 420px; padding-top: 12vh">
    <div class="panel">
      <div class="logo" style="font-size: 24px; margin-bottom: 18px">简<span>播</span></div>
      <div class="page-title">登录面板</div>
      <p class="hint" style="margin-bottom: 18px">服务端已开启访问鉴权，请输入配置中的密码。</p>
      <div class="field">
        <label>访问密码</label>
        <input class="input" type="password" v-model="password" @keyup.enter="submit" placeholder="auth.password" autofocus />
      </div>
      <div v-if="err" class="error" style="margin-bottom: 12px">{{ err }}</div>
      <button class="btn primary" style="width: 100%" :disabled="loading" @click="submit">
        {{ loading ? '登录中…' : '登录' }}
      </button>
    </div>
  </div>
</template>
