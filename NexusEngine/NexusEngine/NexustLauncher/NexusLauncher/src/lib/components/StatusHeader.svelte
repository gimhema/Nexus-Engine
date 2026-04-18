<script lang="ts">
  export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected';

  interface Props {
    status: ConnectionStatus;
    onConnect: () => void;
    onDisconnect: () => void;
  }

  let { status, onConnect, onDisconnect }: Props = $props();

  const statusLabel: Record<ConnectionStatus, string> = {
    disconnected: '연결 끊김',
    connecting:   '연결 중...',
    connected:    '연결됨',
  };
</script>

<header class="header">
  <div class="brand">
    <span class="brand-nexus">NEXUS</span>
    <span class="brand-launcher">LAUNCHER</span>
  </div>

  <div class="status-badge" class:connected={status === 'connected'} class:connecting={status === 'connecting'}>
    <span class="status-dot"></span>
    <span class="status-label">{statusLabel[status]}</span>
  </div>

  <div class="controls">
    {#if status === 'disconnected'}
      <button class="btn btn-primary" onclick={onConnect}>서버에 연결</button>
    {:else if status === 'connecting'}
      <button class="btn" disabled>연결 중...</button>
    {:else}
      <button class="btn btn-danger" onclick={onDisconnect}>연결 해제</button>
    {/if}
  </div>
</header>

<style>
.header {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 0 24px;
  height: 56px;
  background: #0a0a18;
  border-bottom: 1px solid #1e1e3a;
  flex-shrink: 0;
}

.brand {
  display: flex;
  align-items: baseline;
  gap: 6px;
  margin-right: auto;
}

.brand-nexus {
  font-size: 18px;
  font-weight: 700;
  letter-spacing: 3px;
  color: #4fc3f7;
}

.brand-launcher {
  font-size: 11px;
  font-weight: 400;
  letter-spacing: 2px;
  color: #5566aa;
  text-transform: uppercase;
}

/* 상태 배지 */
.status-badge {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 12px;
  border-radius: 20px;
  background: #1a1a30;
  border: 1px solid #2a2a4a;
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #ff4444;
  flex-shrink: 0;
}

.status-badge.connecting .status-dot {
  background: #ffaa00;
  animation: pulse 1s ease-in-out infinite;
}

.status-badge.connected .status-dot {
  background: #44ff88;
  box-shadow: 0 0 6px #44ff8880;
}

.status-label {
  font-size: 12px;
  color: #7788bb;
  letter-spacing: 0.5px;
}

.status-badge.connected .status-label { color: #44ff88; }
.status-badge.connecting .status-label { color: #ffaa00; }

/* 버튼 */
.btn {
  padding: 7px 18px;
  border-radius: 6px;
  border: 1px solid transparent;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
  letter-spacing: 0.3px;
  background: #1e1e3a;
  color: #8899cc;
  border-color: #2a2a50;
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: #1a3a5c;
  color: #4fc3f7;
  border-color: #2a5a8c;
}
.btn-primary:hover:not(:disabled) {
  background: #1e4a72;
  border-color: #4fc3f7;
  box-shadow: 0 0 8px #4fc3f730;
}

.btn-danger {
  background: #3a1a1a;
  color: #ff6666;
  border-color: #5c2a2a;
}
.btn-danger:hover {
  background: #4a1a1a;
  border-color: #ff4444;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50%       { opacity: 0.4; }
}
</style>
