<script lang="ts">
  export interface Player {
    sessionId: number;
    name:      string;
  }

  interface Props {
    players:    Player[];
    connected:  boolean;
    kickingIds: Set<number>;
    onKick:     (sessionId: number, name: string) => void;
  }

  let { players, connected, kickingIds, onKick }: Props = $props();

  // 세션 ID를 16진수 문자열로 포맷
  function fmtSession(id: number): string {
    return '0x' + id.toString(16).toUpperCase().padStart(8, '0');
  }
</script>

<section class="players">
  <div class="section-header">
    <span class="section-title">접속 중인 플레이어</span>
    {#if connected}
      <span class="section-badge">{players.length}명</span>
    {/if}
  </div>

  <div class="list-area">
    {#if !connected}
      <div class="empty-state">
        <span class="empty-icon">🔌</span>
        <span>서버에 연결하면 플레이어 목록이 표시됩니다.</span>
      </div>
    {:else if players.length === 0}
      <div class="empty-state">
        <span class="empty-icon">🌐</span>
        <span>접속 중인 플레이어가 없습니다.</span>
      </div>
    {:else}
      <div class="table">
        <!-- 헤더 -->
        <div class="table-head">
          <span class="col col-index">#</span>
          <span class="col col-name">플레이어</span>
          <span class="col col-session">세션 ID</span>
          <span class="col col-action"></span>
        </div>

        <!-- 행 목록 -->
        <div class="table-body">
          {#each players as player, i (player.sessionId)}
            <div class="table-row">
              <span class="col col-index text-dim">{i + 1}</span>
              <span class="col col-name">{player.name}</span>
              <span class="col col-session text-mono text-dim">{fmtSession(player.sessionId)}</span>
              <span class="col col-action">
                <button
                  class="btn-kick"
                  disabled={kickingIds.has(player.sessionId)}
                  onclick={() => onKick(player.sessionId, player.name)}
                >
                  {kickingIds.has(player.sessionId) ? '처리 중' : '킥'}
                </button>
              </span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</section>

<style>
.players {
  display: flex;
  flex-direction: column;
  flex: 1;
  overflow: hidden;
  padding: 0 24px 16px;
}

/* 섹션 헤더 */
.section-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 0 10px;
  border-bottom: 1px solid #1e1e3a;
  flex-shrink: 0;
}

.section-title {
  font-size: 12px;
  letter-spacing: 1.5px;
  text-transform: uppercase;
  color: #556688;
  font-weight: 500;
}

.section-badge {
  padding: 2px 8px;
  border-radius: 10px;
  background: #1a2a40;
  border: 1px solid #2a4060;
  font-size: 11px;
  color: #4fc3f7;
  font-weight: 600;
}

/* 리스트 영역 */
.list-area {
  flex: 1;
  overflow-y: auto;
  scrollbar-width: thin;
  scrollbar-color: #2a2a4a transparent;
}

.list-area::-webkit-scrollbar       { width: 4px; }
.list-area::-webkit-scrollbar-track  { background: transparent; }
.list-area::-webkit-scrollbar-thumb  { background: #2a2a4a; border-radius: 2px; }

/* 빈 상태 */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  height: 140px;
  color: #3a3a5a;
  font-size: 13px;
}

.empty-icon { font-size: 28px; opacity: 0.4; }

/* 테이블 */
.table {
  display: flex;
  flex-direction: column;
  width: 100%;
}

.table-head {
  display: flex;
  padding: 8px 0;
  border-bottom: 1px solid #1e1e3a;
}

.table-head .col {
  font-size: 10px;
  letter-spacing: 1px;
  text-transform: uppercase;
  color: #44446a;
  font-weight: 600;
}

.table-body { display: flex; flex-direction: column; }

.table-row {
  display: flex;
  align-items: center;
  padding: 9px 0;
  border-bottom: 1px solid #13132a;
  transition: background 0.1s;
}

.table-row:hover { background: #0f0f22; margin: 0 -8px; padding-left: 8px; padding-right: 8px; }

/* 컬럼 너비 */
.col            { display: flex; align-items: center; font-size: 13px; color: #c8d8f0; }
.col-index      { width: 36px;  color: #3a3a5a; flex-shrink: 0; }
.col-name       { flex: 1; min-width: 0; }
.col-session    { width: 160px; flex-shrink: 0; }
.col-action     { width: 72px;  flex-shrink: 0; justify-content: flex-end; }

.text-dim       { color: #445566; }
.text-mono      { font-family: 'Courier New', Courier, monospace; font-size: 11px; letter-spacing: 0.5px; }

/* 킥 버튼 */
.btn-kick {
  padding: 4px 12px;
  border-radius: 4px;
  border: 1px solid #5c2a2a;
  background: #2a1010;
  color: #ff6666;
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s;
  letter-spacing: 0.5px;
}

.btn-kick:hover:not(:disabled) {
  background: #3a1515;
  border-color: #ff4444;
  box-shadow: 0 0 6px #ff444420;
}

.btn-kick:disabled {
  opacity: 0.4;
  cursor: not-allowed;
  color: #886666;
}
</style>
