<script setup lang="ts">
defineProps<{
  badge: string;
  title: string;
  accentColor: string;
  total: number;
  love: number;
  dislike: number;
  disabled: boolean;
  stagger: number;
}>();

const emit = defineEmits<{
  'start-download': [];
  'recover-files': [];
}>();
</script>

<template>
  <v-col cols="12" md="6" class="animate-in" :class="`stagger-${stagger}`">
    <div class="terminal-panel scan-line" :style="{ borderLeft: `2px solid ${accentColor}66` }">
      <div class="panel-header">
        <div class="panel-header-left">
          <div class="hex-badge" :style="{ background: `${accentColor}1f`, color: accentColor }">{{ badge }}</div>
          <div class="panel-header-text">
            <span class="panel-title">{{ title }}</span>
            <span class="panel-subtitle">数据终端</span>
          </div>
        </div>
        <div class="panel-signal" :style="{ background: accentColor, boxShadow: `0 0 6px ${accentColor}` }" />
      </div>

      <div class="panel-stats">
        <div class="stat-cell">
          <span class="stat-number stat-value">{{ total }}</span>
          <span class="stat-label">总计</span>
        </div>
        <div class="stat-divider" />
        <div class="stat-cell">
          <span class="stat-number stat-value" style="color: #10b981">{{ love }}</span>
          <span class="stat-label">可用</span>
        </div>
        <div class="stat-divider" />
        <div class="stat-cell">
          <span class="stat-number stat-value" style="color: #f59e0b">{{ dislike }}</span>
          <span class="stat-label">缺失</span>
        </div>
      </div>

      <div class="panel-divider" />

      <div class="panel-actions">
        <button
          class="panel-action-btn panel-action-btn--primary"
          :disabled="disabled"
          @click="emit('start-download')"
        >
          <v-icon size="14">mdi-download</v-icon>
          <span>开始下载</span>
        </button>
        <button
          class="panel-action-btn panel-action-btn--ghost"
          :disabled="disabled"
          @click="emit('recover-files')"
        >
          <v-icon size="14">mdi-database-sync</v-icon>
          <span>下载所有喜欢的文件</span>
        </button>
      </div>
    </div>
  </v-col>
</template>

<style scoped>
.terminal-panel {
  position: relative;
  background: rgba(var(--surface-card-rgb), 0.55) !important;
  backdrop-filter: blur(16px) saturate(140%);
  -webkit-backdrop-filter: blur(16px) saturate(140%);
  border: var(--border-card);
  border-radius: var(--radius-md);
  overflow: hidden;
  box-shadow: var(--shadow-sm);
}

/* ── 面板头部 ── */
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px 12px;
}
.panel-header-left {
  display: flex;
  align-items: center;
  gap: 8px;
}
.panel-header-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.panel-title {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.875rem;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: 0.03em;
}
.panel-subtitle {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.625rem;
  font-weight: 500;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-tertiary);
}

/* ── 信号指示器 ── */
.panel-signal {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  opacity: 0.6;
  animation: signal-pulse 2s infinite;
}
@keyframes signal-pulse {
  0%, 100% { opacity: 0.6; }
  50% { opacity: 0.3; }
}

/* ── 统计数据行 ── */
.panel-stats {
  display: flex;
  align-items: center;
  padding: 4px 16px 16px;
}
.stat-cell {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}
.stat-value {
  font-size: 1.75rem;
  font-weight: 700;
  color: var(--text-primary);
}
.stat-divider {
  width: 1px;
  height: 32px;
  background: var(--border-subtle);
  flex-shrink: 0;
}

/* ── 面板分隔线 ── */
.panel-divider {
  height: 1px;
  background: linear-gradient(90deg, var(--accent-primary) 0%, var(--border-subtle) 30%, transparent 100%);
  opacity: 0.3;
}

/* ── 操作按钮 ── */
.panel-actions {
  display: flex;
  gap: 8px;
  padding: 12px 16px;
}
.panel-action-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 14px;
  border: none;
  border-radius: var(--radius-sm);
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.75rem;
  font-weight: 600;
  letter-spacing: 0.03em;
  cursor: pointer;
  transition: all 0.2s var(--ease-out);
}
.panel-action-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.panel-action-btn--primary {
  background: linear-gradient(135deg, var(--accent-primary) 0%, #2563eb 100%);
  color: #fff;
}
.panel-action-btn--primary:hover:not(:disabled) {
  box-shadow: var(--shadow-glow-strong);
  transform: translateY(-1px);
}
.panel-action-btn--primary:active:not(:disabled) {
  transform: scale(0.97);
}
.panel-action-btn--ghost {
  background: transparent;
  color: var(--text-secondary);
  border: var(--border-card);
}
.panel-action-btn--ghost:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-primary);
  border-color: var(--border-default);
}
</style>
