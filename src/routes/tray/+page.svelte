<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";

  const MAX_ITEMS = 5;

  let title       = $state("Nothing playing");
  let artist      = $state("");
  let thumbnail   = $state("");
  let liked       = $state(false);
  let disliked    = $state(false);
  let playing     = $state(false);
  let volume      = $state(100);
  let prevVolume  = 100;
  let currentTime = $state(0);
  let duration    = $state(0);
  let queue       = $state([]);
  let showQueue   = $state(false);
  let showVolume  = $state(false);
  let isSeeking      = $state(false);
  let seekValue      = $state(0);
  let isVolAdjusting = false;
  let seekTimeout;
  let volTimeout;
  let unlisten;


  onMount(async () => {
    unlisten = await listen("player_state_changed", (e) => {
      const p = e.payload;
      const newTitle   = p.title    || "Nothing playing";
      const newDur     = p.duration || 0;
      const newTime    = p.currentTime || 0;

      // Detect song change by title, OR by a big duration jump, OR by time
      // jumping backward without a user seek (covers same-title back-to-back songs)
      const titleChanged    = newTitle !== title;
      const durationChanged = duration > 0 && Math.abs(newDur - duration) > 5;
      const timeJumped      = !isSeeking && currentTime > 5 && newTime < currentTime - 5;
      const songChanged     = titleChanged || durationChanged || timeJumped;

      if (songChanged) {
        isSeeking = false;
        clearTimeout(seekTimeout);
        currentTime = 0;
        seekValue   = 0;
        duration    = 0;
      } else if (!isSeeking) {
        currentTime = newTime;
        seekValue   = newTime;
      }

      title     = newTitle;
      artist    = p.artist    || "";
      thumbnail = p.thumbnail || "";
      liked     = p.liked     ?? false;
      disliked  = p.disliked  ?? false;
      playing   = p.playing;
      if (!isVolAdjusting) volume = p.volume ?? volume;
      if (!songChanged) duration = newDur;
      queue     = p.queue     || [];
    });
  });

  onDestroy(() => { unlisten?.(); clearTimeout(seekTimeout); clearTimeout(volTimeout); });

  function fmt(secs) {
    if (!secs || isNaN(secs)) return "0:00";
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60).toString().padStart(2, "0");
    return `${m}:${s}`;
  }

  const control  = (action) => invoke("player_control", { action });
  const openApp  = () => { invoke("show_main_window"); invoke("hide_tray_popup"); };
  const close    = () => invoke("hide_tray_popup");

  function onSeekInput(e) { isSeeking = true; seekValue = +e.target.value; }
  function onSeekCommit(e) {
    const pos = +e.target.value;
    invoke("player_seek", { position: pos });
    // Stay in seeking mode for 1.5s so stale poll updates don't snap the bar back
    // before YTM has applied the new position
    clearTimeout(seekTimeout);
    seekTimeout = setTimeout(() => { isSeeking = false; }, 1500);
  }

  const BASE_H   = 205;
  const VOL_H    = 24;
  const QUEUE_H  = 210;

  function syncSize() {
    const h = BASE_H + (showVolume ? VOL_H : 0) + (showQueue ? QUEUE_H : 0);
    invoke("resize_popup", { height: h });
  }

  function onVolumeInput(e) {
    volume = +e.target.value;
    isVolAdjusting = true;
    clearTimeout(volTimeout);
    volTimeout = setTimeout(() => { isVolAdjusting = false; }, 2000);
    invoke("player_volume", { volume });
  }

  function toggleMute() {
    if (volume > 0) { prevVolume = volume; volume = 0; }
    else             { volume = prevVolume || 100; }
    invoke("player_volume", { volume });
  }

  function toggleVolume() {
    showVolume = !showVolume;
    syncSize();
  }

  function toggleQueue() {
    showQueue = !showQueue;
    syncSize();
  }

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
    {#if thumbnail}
      <img class="thumb" src={thumbnail} alt="" draggable="false" />
    {:else}
      <div class="thumb thumb-placeholder">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" opacity="0.3">
          <path d="M12 3v10.55A4 4 0 1014 17V7h4V3h-6z"/>
        </svg>
      </div>
    {/if}
    <div class="info-text">
      <p class="title" title={title}>{title}</p>
      <p class="artist" title={artist}>{artist}</p>
    </div>
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

    <div class="secondary-controls">
      <button
        class="like-btn"
        class:active={liked}
        onclick={() => control("like")}
        aria-label="Like"
        title="Like"
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
          <path d="M1 21h4V9H1v12zm22-11c0-1.1-.9-2-2-2h-6.31l.95-4.57.03-.32c0-.41-.17-.79-.44-1.06L14.17 1 7.59 7.59C7.22 7.95 7 8.45 7 9v10c0 1.1.9 2 2 2h9c.83 0 1.54-.5 1.84-1.22l3.02-7.05c.09-.23.14-.47.14-.73v-2z"/>
        </svg>
      </button>
      <button
        class="like-btn dislike-btn"
        class:active={disliked}
        onclick={() => control("dislike")}
        aria-label="Dislike"
        title="Dislike"
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
          <path d="M15 3H6c-.83 0-1.54.5-1.84 1.22l-3.02 7.05c-.09.23-.14.47-.14.73v2c0 1.1.9 2 2 2h6.31l-.95 4.57-.03.32c0 .41.17.79.44 1.06L9.83 23l6.59-6.59c.36-.36.58-.86.58-1.41V5c0-1.1-.9-2-2-2zm4 0v12h4V3h-4z"/>
        </svg>
      </button>
      <button
        class="like-btn"
        class:active={showVolume}
        onclick={toggleVolume}
        aria-label="Volume"
        title="Volume"
      >
        {#if volume === 0}
          <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
            <path d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z"/>
          </svg>
        {:else if volume < 50}
          <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
            <path d="M18.5 12c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM5 9v6h4l5 5V4L9 9H5z"/>
          </svg>
        {:else}
          <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
            <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"/>
          </svg>
        {/if}
      </button>
      {#if queue.length > 1}
        <button class="queue-toggle" onclick={toggleQueue} aria-label="Toggle queue" title="Queue">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
            <path d="M3 6h18v2H3zm0 5h18v2H3zm0 5h12v2H3z"/>
          </svg>
          <span class="queue-count">{queue.length}</span>
        </button>
      {/if}
    </div>
  </section>

  <!-- ── Volume (expandable) ── -->
  {#if showVolume}
    <section class="volume-section">
      <button class="vol-mute" onclick={toggleMute} aria-label={volume === 0 ? "Unmute" : "Mute"} title={volume === 0 ? "Unmute" : "Mute"}>
        <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
          {#if volume === 0}
            <path d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z"/>
          {:else}
            <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02z"/>
          {/if}
        </svg>
      </button>
      <input
        type="range"
        class="vol-range"
        min="0"
        max="100"
        step="1"
        value={volume}
        style="--pct: {volume}%"
        oninput={onVolumeInput}
        aria-label="Volume"
      />
      <span class="vol-pct">{volume}%</span>
    </section>
  {/if}

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
  :global(html), :global(body) {
    background: #1c1c1e;
    overflow: hidden;
    height: 100%;
    width: 100%;
  }

  main {
    width: 100%;
    min-height: 100%;
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
  .info {
    display: flex; align-items: center; gap: 10px; overflow: hidden;
  }
  .thumb {
    width: 52px; height: 52px; border-radius: 7px;
    object-fit: cover; flex-shrink: 0;
  }
  .thumb-placeholder {
    background: rgba(255,255,255,0.06);
    display: flex; align-items: center; justify-content: center;
    color: #f5f5f7;
  }
  .info-text { flex: 1; overflow: hidden; }
  .title {
    font-size: 13px; font-weight: 600;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    line-height: 1.3;
  }
  .artist {
    font-size: 11px; color: #8e8e93; margin-top: 2px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }

  /* Progress */
  .progress-section { display: flex; flex-direction: column; gap: 3px; }
  .track-wrap { position: relative; height: 16px; display: flex; align-items: center; }

  input[type=range] {
    -webkit-appearance: none;
    appearance: none;
    width: 100%; height: 3px; border-radius: 3px; outline: none; cursor: pointer;
    background: linear-gradient(
      to right,
      #a855f7 0%,
      #3b82f6 var(--pct),
      rgba(255,255,255,0.12) var(--pct),
      rgba(255,255,255,0.12) 100%
    );
    border: none;
    padding: 0;
    margin: 0;
  }
  input[type=range]::-webkit-slider-runnable-track {
    height: 3px; border-radius: 3px; background: transparent;
  }
  input[type=range]::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 11px; height: 11px; border-radius: 50%;
    background: #fff; box-shadow: 0 0 4px rgba(0,0,0,0.4);
    transition: transform 0.1s;
    margin-top: -4px;
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

  .secondary-controls {
    margin-left: auto;
    display: flex; align-items: center; gap: 0;
  }
  .like-btn { color: #636366; padding: 6px; }
  .like-btn:hover { color: #f5f5f7; }
  .like-btn.active { color: #c026d3; }
  .dislike-btn.active { color: #8e8e93; }

  .queue-toggle {
    color: #636366;
    display: flex; align-items: center; gap: 4px;
  }
  .queue-toggle:hover { color: #f5f5f7; }
  .queue-count { font-size: 10px; font-weight: 600; }

  /* Volume */
  .volume-section {
    display: flex; align-items: center; gap: 7px; padding: 0 1px;
  }
  .vol-mute {
    color: #636366; padding: 3px; flex-shrink: 0; background: none; border: none;
    cursor: pointer; display: flex; align-items: center; border-radius: 4px;
    transition: color 0.15s;
  }
  .vol-mute:hover { color: #f5f5f7; }
  .vol-range {
    flex: 1; height: 3px;
    background: linear-gradient(
      to right,
      #a855f7 0%,
      #3b82f6 var(--pct),
      rgba(255,255,255,0.12) var(--pct),
      rgba(255,255,255,0.12) 100%
    );
  }
  .vol-pct {
    font-size: 10px; color: #636366; width: 28px; text-align: right; flex-shrink: 0;
  }

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
