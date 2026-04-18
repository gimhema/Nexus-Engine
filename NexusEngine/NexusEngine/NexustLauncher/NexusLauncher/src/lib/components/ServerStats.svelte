<script lang="ts">
  interface Props {
    playerCount:   number;
    uptimeSeconds: number;
    connected:     boolean;
  }

  let { playerCount, uptimeSeconds, connected }: Props = $props();

  function formatUptime(sec: number): string {
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    const s = sec % 60;
    return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  }
</script>

<section class="stats">
  <div class="stat-card">
    <span class="stat-icon">👥</span>
    <div class="stat-body">
      <span class="stat-label">접속자 수</span>
      <span class="stat-value" class:dim={!connected}>
        {connected ? playerCount : '—'}<span class="stat-unit">{connected ? '명' : ''}</span>
      </span>
    </div>
  </div>

  <div class="stat-divider"></div>

  <div class="stat-card">
    <span class="stat-icon">⏱</span>
    <div class="stat-body">
      <span class="stat-label">가동 시간</span>
      <span class="stat-value mono" class:dim={!connected}>
        {connected ? formatUptime(uptimeSeconds) : '—'}
      </span>
    </div>
  </div>
</section>

<style>
.stats {
  display: flex;
  align-items: center;
  padding: 0 24px;
  height: 88px;
  background: #0d0d20;
  border-bottom: 1px solid #1e1e3a;
  gap: 0;
  flex-shrink: 0;
}

.stat-card {
  display: flex;
  align-items: center;
  gap: 14px;
  flex: 1;
}

.stat-icon {
  font-size: 22px;
  opacity: 0.7;
}

.stat-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.stat-label {
  font-size: 11px;
  color: #556688;
  letter-spacing: 1px;
  text-transform: uppercase;
}

.stat-value {
  font-size: 26px;
  font-weight: 300;
  color: #c8d8f0;
  line-height: 1;
}

.stat-value.mono {
  font-family: 'Courier New', Courier, monospace;
  font-size: 22px;
  letter-spacing: 2px;
}

.stat-unit {
  font-size: 14px;
  color: #556688;
  margin-left: 3px;
}

.stat-value.dim {
  color: #3a3a5a;
}

.stat-divider {
  width: 1px;
  height: 48px;
  background: #1e1e3a;
  margin: 0 32px;
  flex-shrink: 0;
}
</style>
