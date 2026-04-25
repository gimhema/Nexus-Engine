<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  export interface ServerProcess {
    pid:  number;
    name: string;
  }

  // ─── 상태 ─────────────────────────────────────────────────────────────────
  let processes    = $state<ServerProcess[]>([]);
  let loading      = $state(false);
  let killing      = $state(false);
  let lastRefresh  = $state<Date | null>(null);
  let killResult   = $state<string | null>(null);

  // ─── 프로세스 목록 조회 ───────────────────────────────────────────────────
  async function refresh() {
    loading = true;
    try {
      processes   = await invoke<ServerProcess[]>('list_server_processes');
      lastRefresh = new Date();
    } catch (e) {
      console.error('프로세스 목록 조회 실패:', e);
    } finally {
      loading = false;
    }
  }

  // ─── 전체 강제 종료 ───────────────────────────────────────────────────────
  async function killAll() {
    killing     = true;
    killResult  = null;
    try {
      const result = await invoke<{ killed: number }>('kill_server_processes');
      killResult = `${result.killed}개 프로세스 종료됨`;
      // 종료 후 목록 갱신
      await refresh();
    } catch (e) {
      console.error('프로세스 종료 실패:', e);
      killResult = '종료 실패';
    } finally {
      killing = false;
      // 3초 후 메시지 초기화
      setTimeout(() => { killResult = null; }, 3000);
    }
  }

  // ─── 마운트 시 즉시 조회 ──────────────────────────────────────────────────
  import { onMount } from 'svelte';
  onMount(refresh);
</script>

<section class="process-manager">
  <header class="pm-header">
    <span class="pm-title">서버 프로세스</span>
    <div class="pm-actions">
      {#if killResult}
        <span class="kill-result">{killResult}</span>
      {/if}
      <button class="btn-kill" onclick={killAll} disabled={killing || processes.length === 0}>
        {killing ? '종료 중…' : '전체 종료'}
      </button>
      <button class="btn-refresh" onclick={refresh} disabled={loading}>
        {loading ? '…' : '↻'}
      </button>
    </div>
  </header>

  {#if processes.length === 0}
    <div class="pm-empty">실행 중인 서버 없음</div>
  {:else}
    <ul class="pm-list">
      {#each processes as p (p.pid)}
        <li class="pm-item">
          <span class="pm-name">{p.name}</span>
          <span class="pm-pid">PID {p.pid}</span>
        </li>
      {/each}
    </ul>
  {/if}

  {#if lastRefresh}
    <div class="pm-footer">
      갱신: {lastRefresh.toLocaleTimeString()}
    </div>
  {/if}
</section>

<style>
  .process-manager {
    display: flex;
    flex-direction: column;
    background: #131326;
    border-top: 1px solid #2a2a4a;
    padding: 10px 16px;
    gap: 6px;
  }

  .pm-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .pm-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: #7090c0;
  }

  .pm-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .kill-result {
    font-size: 11px;
    color: #80c8a0;
    animation: fadeOut 3s forwards;
  }

  @keyframes fadeOut {
    0%, 70% { opacity: 1; }
    100%     { opacity: 0; }
  }

  button {
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 11px;
    cursor: pointer;
    border: 1px solid transparent;
    transition: background 0.15s, opacity 0.15s;
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .btn-kill {
    background: #3a1a1a;
    border-color: #6a2020;
    color: #e08080;
  }

  .btn-kill:not(:disabled):hover {
    background: #501a1a;
  }

  .btn-refresh {
    background: #1a1a3a;
    border-color: #3a3a6a;
    color: #8090c0;
  }

  .btn-refresh:not(:disabled):hover {
    background: #252550;
  }

  .pm-empty {
    font-size: 12px;
    color: #405070;
    text-align: center;
    padding: 4px 0;
  }

  .pm-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .pm-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 3px 6px;
    border-radius: 4px;
    background: #1a1a30;
  }

  .pm-name {
    font-size: 12px;
    color: #90b8e0;
  }

  .pm-pid {
    font-size: 11px;
    color: #506080;
    font-variant-numeric: tabular-nums;
  }

  .pm-footer {
    font-size: 10px;
    color: #384858;
    text-align: right;
  }
</style>
