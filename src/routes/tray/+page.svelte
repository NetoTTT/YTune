<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { LogicalSize } from "@tauri-apps/api/dpi";
  import { onMount, onDestroy } from "svelte";

  const COMPACT_H  = 195;
  const QUEUE_HEAD = 40;
  const ITEM_H     = 44;
  const MAX_ITEMS  = 5;
  const WIDTH      = 340;

  let title       = $state("Nothing playing");
  let artist      = $state("");
  let playing     = $state(false);
  let currentTime = $state(0);
  let duration    = $state(0);
  let queue       = $state([]);
  let showQueue   = $state(false);
  let isSeeking   = $state(false);
  let seekValue   = $state(0);
  let unlisten;

  $effect(() => {
    if (!isSeeking) seekValue = currentTime;
  });

  $effect(() => {
    resizeWindow();
  });

  async function resizeWindow() {
    try {
      const items = Math.min(queue.length, MAX_ITEMS);
      const h = showQueue && items > 0
        ? COMPACT_H + QUEUE_HEAD + items * ITEM_H
        : COMPACT_H;
      const win = getCurrentWindow();
      await win.setSize(new LogicalSize(WIDTH, h));
    } catch {}
  }

  onMount(async () => {
    unlisten = await listen("player_state_changed", (e) => {
      const p = e.payload;
      title   = p.title  || "Nothing playing";
      artist  = p.artist || "";
      playing = p.playing;
      duration = p.duration || 0;
      queue   = p.queue || [];
      if (!isSeeking) currentTime = p.currentTime || 0;
      // Auto-open queue if a playlist is detected and the popup is fresh
    });
  });

  onDestroy(() => unlisten?.());

  function fmt(secs) {
    if (!secs || isNaN(secs)) return "0:00";
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60).toString().padStart(2, "0");
    return `${m}:${s}`;
  }

  const control  = (action) => invoke("player_control", { action });
  const openApp  = () => { invoke("show_main_window"); invoke("hide_tray_popup"); };
  const close    = () => invoke("hide_tray_popup");

  function onSeekInput(e)  { isSeeking = true; seekValue = +e.target.value; }
  function onSeekCommit(e) { isSeeking = false; invoke("player_seek", { position: +e.target.value }); }

  function toggleQueue() { showQueue = !showQueue; }

  // Progress percentage for CSS gradient on the range track
  const progressPct = $derived(
    duration > 0 ? ((isSeeking ? seekValue : currentTime) / duration) * 100 : 0
  );
</script>

<main data-tauri-drag-region>

  <!-- ── Header ── -->
  <header data-tauri-drag-region>
    <span class="brand">ytune</span>
    <div class="header-actions">
      <button onclick={openApp} aria-label="Open app" title="Open ytune">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/>
          <polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/>
        </svg>
      </button>
      <button onclick={close} aria-label="Close" class="close">
        <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
          <path d="M1 1l10 10M11 1L1 11" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
        </svg>
      </button>
    </div>
  </header>

  <!-- ── Song info ── -->
  <section class="info">
    <p class="title" title={title}>{title}</p>
    <p class="artist" title={artist}>{artist}</p>
  </section>

  <!-- ── Progress bar ── -->
  <section class="progress-section">
    <div class="track-wrap">
      <input
        type="range"
        min="0"
        max={duration || 1}
        step="1"
        value={isSeeking ? seekValue : currentTime}
        style="--pct: {progressPct}%"
        oninput={onSeekInput}
        onchange={onSeekCommit}
        aria-label="Seek"
      />
    </div>
    <div class="times">
      <span>{fmt(isSeeking ? seekValue : currentTime)}</span>
      <span>{fmt(duration)}</span>
    </div>
  </section>

  <!-- ── Controls ── -->
  <section class="controls">
    <button onclick={() => control("previous")} aria-label="Previous">
      <svg width="19" height="19" viewBox="0 0 24 24" fill="currentColor">
        <path d="M6 6h2v12H6zm3.5 6 8.5 6V6z"/>
      </svg>
    </button>

    <button class="play" onclick={() => control("play_pause")} aria-label={playing ? "Pause" : "Play"}>
      {#if playing}
        <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor">
          <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/>
        </svg>
      {:else}
        <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor">
          <path d="M8 5v14l11-7z"/>
        </svg>
      {/if}
    </button>

    <button onclick={() => control("next")} aria-label="Next">
      <svg width="19" height="19" viewBox="0 0 24 24" fill="currentColor">
        <path d="M6 18l8.5-6L6 6v12zm10-12v12h2V6h-2z"/>
      </svg>
    </button>

    {#if queue.length > 1}
      <button class="queue-toggle" onclick={toggleQueue} aria-label="Toggle queue" title="Queue">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
          <path d="M3 6h18v2H3zm0 5h18v2H3zm0 5h12v2H3z"/>
        </svg>
        <span class="queue-count">{queue.length}</span>
      </button>
    {/if}
  </section>

  <!-- ── Queue ── -->
  {#if showQueue && queue.length > 1}
    <section class="queue">
      <p class="queue-label">Queue</p>
      <ul>
        {#each queue.slice(0, MAX_ITEMS) as item}
          <li class:current={item.current}>
            <span class="dot">{item.current ? "▶" : "·"}</span>
            <span class="q-title" title={item.title}>{item.title}</span>
            <span class="q-artist" title={item.artist}>{item.artist}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

</main>

<style>
  :global(*) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(body) { background: transparent; overflow: hidden; }

  main {
    width: 340px;
    background: #1c1c1e;
    color: #f5f5f7;
    border-radius: 14px;
    border: 1px solid rgba(255,255,255,0.07);
    padding: 13px 15px 12px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    font-family: -apple-system, "Segoe UI", system-ui, sans-serif;
    user-select: none;
    box-shadow: 0 12px 40px rgba(0,0,0,0.6);
  }

  /* Header */
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .brand {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    background: linear-gradient(90deg, #c026d3, #3b82f6);
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    background-clip: text;
  }
  .header-actions { display: flex; gap: 2px; }
  .header-actions button {
    background: none; border: none; cursor: pointer;
    color: #666; padding: 3px 4px; border-radius: 5px;
    display: flex; align-items: center; transition: color 0.15s;
  }
  .header-actions button:hover { color: #f5f5f7; }
  .header-actions .close:hover { color: #ff453a; }

  /* Song info */
  .info { overflow: hidden; }
  .title {
    font-size: 14px; font-weight: 600;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    line-height: 1.3;
  }
  .artist {
    font-size: 12px; color: #8e8e93; margin-top: 2px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }

  /* Progress */
  .progress-section { display: flex; flex-direction: column; gap: 3px; }
  .track-wrap { position: relative; height: 16px; display: flex; align-items: center; }

  input[type=range] {
    -webkit-appearance: none;
    width: 100%; height: 3px; border-radius: 3px; outline: none; cursor: pointer;
    background: linear-gradient(
      to right,
      #a855f7 0%,
      #3b82f6 var(--pct),
      rgba(255,255,255,0.12) var(--pct),
      rgba(255,255,255,0.12) 100%
    );
  }
  input[type=range]::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 11px; height: 11px; border-radius: 50%;
    background: #fff; box-shadow: 0 0 4px rgba(0,0,0,0.4);
    transition: transform 0.1s;
  }
  input[type=range]:hover::-webkit-slider-thumb { transform: scale(1.3); }

  .times {
    display: flex; justify-content: space-between;
    font-size: 10px; color: #636366;
  }

  /* Controls */
  .controls { display: flex; align-items: center; gap: 2px; }
  button {
    background: none; border: none; cursor: pointer;
    color: #f5f5f7; padding: 7px; border-radius: 8px;
    display: flex; align-items: center; justify-content: center;
    transition: background 0.15s;
  }
  button:hover  { background: rgba(255,255,255,0.09); }
  button:active { background: rgba(255,255,255,0.05); }

  .play {
    background: rgba(168,85,247,0.18);
    background: linear-gradient(135deg,rgba(192,38,211,0.2),rgba(59,130,246,0.2));
    padding: 9px; margin: 0 3px; border-radius: 50%;
  }
  .play:hover {
    background: linear-gradient(135deg,rgba(192,38,211,0.35),rgba(59,130,246,0.35));
  }

  .queue-toggle {
    margin-left: auto; color: #636366;
    display: flex; align-items: center; gap: 4px;
  }
  .queue-toggle:hover { color: #f5f5f7; }
  .queue-count { font-size: 10px; font-weight: 600; }

  /* Queue section */
  .queue {
    border-top: 1px solid rgba(255,255,255,0.06);
    padding-top: 8px;
  }
  .queue-label {
    font-size: 10px; font-weight: 600; text-transform: uppercase;
    letter-spacing: 0.08em; color: #636366; margin-bottom: 5px;
  }
  ul { list-style: none; display: flex; flex-direction: column; gap: 1px; }
  li {
    display: grid;
    grid-template-columns: 14px 1fr auto;
    align-items: center;
    gap: 6px;
    padding: 6px 6px 6px 4px;
    border-radius: 6px;
    transition: background 0.12s;
  }
  li:hover { background: rgba(255,255,255,0.05); }
  li.current { background: rgba(168,85,247,0.12); }

  .dot {
    font-size: 10px; color: #636366; text-align: center; line-height: 1;
  }
  li.current .dot { color: #a855f7; }

  .q-title {
    font-size: 12px; font-weight: 500;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  li.current .q-title { color: #c084fc; }

  .q-artist {
    font-size: 11px; color: #636366;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    max-width: 90px; text-align: right;
  }
</style>
