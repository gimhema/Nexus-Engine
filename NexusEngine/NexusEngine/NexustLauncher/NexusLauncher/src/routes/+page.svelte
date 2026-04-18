<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';

  import StatusHeader, { type ConnectionStatus } from '$lib/components/StatusHeader.svelte';
  import ServerStats from '$lib/components/ServerStats.svelte';
  import PlayerList,  { type Player }            from '$lib/components/PlayerList.svelte';

  // ─── 상태 ─────────────────────────────────────────────────────────────────
  let status        = $state<ConnectionStatus>('disconnected');
  let playerCount   = $state(0);
  let uptimeSeconds = $state(0);
  let players       = $state<Player[]>([]);
  let kickingIds    = $state<Set<number>>(new Set());

  // 상태 폴링 타이머 (Arbiter 연결 중에만 동작)
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  // ─── Arbiter 연결 ─────────────────────────────────────────────────────────
  async function connect() {
    if (status !== 'disconnected') return;
    status = 'connecting';
    try {
      await invoke('connect_arbiter');
    } catch (e) {
      console.error('연결 실패:', e);
      status = 'disconnected';
    }
  }

  async function disconnect() {
    stopPoll();
    await invoke('disconnect_arbiter');
    resetState();
  }

  // ─── 상태 폴링 ────────────────────────────────────────────────────────────
  function startPoll() {
    // 연결 직후 즉시 1회 요청
    invoke('get_server_status').catch(console.error);
    // 이후 5초 간격 폴링
    pollTimer = setInterval(() => {
      invoke('get_server_status').catch(console.error);
    }, 5000);
  }

  function stopPoll() {
    if (pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }

  // ─── 킥 ──────────────────────────────────────────────────────────────────
  async function kickPlayer(sessionId: number, name: string) {
    kickingIds = new Set([...kickingIds, sessionId]);
    try {
      const result = await invoke<{ success: boolean; message: string }>('kick_player', {
        sessionId,
        reason: '런처에서 킥됨',
      });
      if (!result.success) {
        console.warn(`킥 실패 (${name}):`, result.message);
      }
    } catch (e) {
      console.error('킥 오류:', e);
    } finally {
      kickingIds = new Set([...kickingIds].filter(id => id !== sessionId));
    }
  }

  // ─── 상태 초기화 ──────────────────────────────────────────────────────────
  function resetState() {
    status        = 'disconnected';
    players       = [];
    playerCount   = 0;
    uptimeSeconds = 0;
    kickingIds    = new Set();
  }

  // ─── Tauri 이벤트 리스너 등록 ─────────────────────────────────────────────
  const unlisteners: UnlistenFn[] = [];

  onMount(async () => {
    // Arbiter 인증 완료 → 연결 확정 + 폴링 시작
    unlisteners.push(await listen('arbiter_connected', () => {
      status = 'connected';
      startPoll();
    }));

    // Arbiter 연결 해제 (서버 종료 / 오류)
    unlisteners.push(await listen('arbiter_disconnected', () => {
      stopPoll();
      resetState();
    }));

    // 서버 상태 업데이트 (AMSG_STATUS)
    unlisteners.push(await listen<{ player_count: number; uptime_seconds: number }>(
      'server_status',
      (event) => {
        playerCount   = event.payload.player_count;
        uptimeSeconds = event.payload.uptime_seconds;
      }
    ));

    // 플레이어 접속 이벤트 (AMSG_EVENT_PLAYER_JOIN)
    unlisteners.push(await listen<{ session_id: number; name: string }>(
      'player_joined',
      (event) => {
        const { session_id, name } = event.payload;
        // 중복 방지
        if (!players.some(p => p.sessionId === session_id)) {
          players = [...players, { sessionId: session_id, name }];
        }
      }
    ));

    // 플레이어 퇴장 이벤트 (AMSG_EVENT_PLAYER_LEAVE)
    unlisteners.push(await listen<{ session_id: number }>(
      'player_left',
      (event) => {
        players = players.filter(p => p.sessionId !== event.payload.session_id);
      }
    ));

    // 서버 준비 이벤트 (AMSG_EVENT_SERVER_READY)
    unlisteners.push(await listen('server_ready', () => {
      if (status === 'connected') {
        invoke('get_server_status').catch(console.error);
      }
    }));
  });

  // 컴포넌트 언마운트 시 리스너 + 폴링 정리
  onDestroy(() => {
    stopPoll();
    unlisteners.forEach(fn => fn());
  });
</script>

<div class="launcher">
  <StatusHeader {status} onConnect={connect} onDisconnect={disconnect} />
  <ServerStats {playerCount} {uptimeSeconds} connected={status === 'connected'} />
  <PlayerList {players} connected={status === 'connected'} {kickingIds} onKick={kickPlayer} />
</div>

<style>
  :global(*, *::before, *::after) {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
  }

  :global(html, body) {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #0d0d20;
    color: #c8d8f0;
    font-family: 'Segoe UI', system-ui, -apple-system, sans-serif;
    font-size: 14px;
    line-height: 1.4;
    -webkit-font-smoothing: antialiased;
  }

  .launcher {
    display: flex;
    flex-direction: column;
    width: 100vw;
    height: 100vh;
    background: #0d0d20;
    overflow: hidden;
  }
</style>
