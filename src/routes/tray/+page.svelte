<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, onDestroy, tick } from "svelte";

  const MAX_ITEMS = 5;

  const THEMES = [
    { name: 'Purple',  h: 280, s: 65 },
    { name: 'Blue',    h: 218, s: 72 },
    { name: 'Cyan',    h: 190, s: 65 },
    { name: 'Green',   h: 150, s: 55 },
    { name: 'Yellow',  h: 48,  s: 80 },
    { name: 'Orange',  h: 25,  s: 80 },
    { name: 'Red',     h: 4,   s: 68 },
    { name: 'Pink',    h: 328, s: 65 },
  ];

  let title         = $state("Nothing playing");
  let artist        = $state("");
  let thumbnail     = $state("");
  let thumbnailData = $state("");
  let liked         = $state(false);
  let disliked    = $state(false);
  let playing     = $state(false);
  let volume      = $state(100);
  let prevVolume  = 100;
  let currentTime = $state(0);
  let duration    = $state(0);
  let queue       = $state([]);
  let showQueue   = $state(false);
  let showVolume  = $state(false);
  let showConfig      = $state(false);
  let discordEnabled  = $state(true);
  let colorMode    = $state("dynamic"); // "dynamic" | "fixed"
  let fixedTheme   = $state(0);         // index into THEMES
  let bgBase       = $state("art");     // "solid" | "art"  (background image)
  let bgViz        = $state("none");    // "none" | "cava" | "spectrum"
  let vizColorMode = $state("dynamic"); // "dynamic" | "fixed"  (bar color for cava/spectrum)
  let lastPalettes     = [{ h: 280, s: 65 }];
  let currentPalette  = $state({ h: 280, s: 65 });
  let cycleMode       = $state("none"); // "none" | "cycle"
  // let crossfade       = $state(0);      // crossfade disabled
  let cycleAnimFrame  = null;
  let crossAnimFrame  = null;
  let isSeeking      = $state(false);
  let seekValue      = $state(0);
  let isVolAdjusting = false;
  let seekTimeout;
  let volTimeout;
  let unlisten;
  let unlistenViz;

  // ── Visualizer canvas ─────────────────────────────────────────────
  let vizCanvas = null; // set in onMount via querySelector — bind:this unreliable in runes mode
  let vizFrame;
  const VIZ_N   = 24;
  let vizBars    = Array(VIZ_N).fill(0);
  let vizTargets = Array(VIZ_N).fill(0);
  let vizPhase   = 0;
  let rawBars    = Array(32).fill(0); // real FFT data from Web Audio API

  // ── Palette ───────────────────────────────────────────────────────
  // Colors are extracted in the injection script (music.youtube.com context)
  // and arrive here as paletteH / paletteS in the state payload.
  // Setting all vars directly avoids relying on the browser to re-evaluate
  // chained custom-property references when an ancestor var changes.
  function applyPalette(h, s) {
    const S = Math.max(45, s);
    const H2 = (h + 35) % 360;
    const root = document.documentElement;
    root.style.setProperty('--h',          String(h));
    root.style.setProperty('--s',          `${S}%`);
    root.style.setProperty('--accent',     `hsl(${h},${S}%,62%)`);
    root.style.setProperty('--accent2',    `hsl(${H2},${S}%,62%)`);
    root.style.setProperty('--accent-dim', `hsla(${h},${S}%,45%,0.22)`);
    root.style.setProperty('--grad',       `linear-gradient(135deg,hsl(${h},${S}%,62%),hsl(${H2},${S}%,62%))`);
  }

  function lerpHue(a, b, t) {
    let diff = b - a;
    if (diff > 180) diff -= 360;
    if (diff < -180) diff += 360;
    return ((a + diff * t) % 360 + 360) % 360;
  }

  function smoothToColor(targetH, targetS, onDone) {
    if (crossAnimFrame) cancelAnimationFrame(crossAnimFrame);
    if (cycleAnimFrame) { cancelAnimationFrame(cycleAnimFrame); cycleAnimFrame = null; }
    const S = Math.max(45, targetS);
    const fromH = currentPalette.h;
    const fromS = currentPalette.s;
    if (fromH === targetH && fromS === S) { if (onDone) onDone(); return; }
    const start = performance.now();
    const DURATION = 800;
    function tick() {
      const t = Math.min((performance.now() - start) / DURATION, 1);
      const ease = 1 - Math.pow(1 - t, 3);
      const h = lerpHue(fromH, targetH, ease);
      const s = fromS + (S - fromS) * ease;
      currentPalette = { h, s: Math.max(45, s) };
      applyPalette(currentPalette.h, currentPalette.s);
      if (t < 1) crossAnimFrame = requestAnimationFrame(tick);
      else { currentPalette = { h: targetH, s: S }; applyPalette(targetH, S); if (onDone) onDone(); }
    }
    crossAnimFrame = requestAnimationFrame(tick);
  }

  function startCycle(palettes) {
    if (cycleAnimFrame) cancelAnimationFrame(cycleAnimFrame);
    const SEGMENT_MS = 3000;
    const startedAt = performance.now();
    function tick() {
      const total = SEGMENT_MS * palettes.length;
      const elapsed = (performance.now() - startedAt) % total;
      const segMs = total / palettes.length;
      const seg = Math.floor(elapsed / segMs);
      const t0 = (elapsed % segMs) / segMs;
      const t = t0 < 0.5 ? 2 * t0 * t0 : 1 - Math.pow(-2 * t0 + 2, 2) / 2;
      const from = palettes[seg];
      const to = palettes[(seg + 1) % palettes.length];
      const h = lerpHue(from.h, to.h, t);
      const s = from.s + (to.s - from.s) * t;
      currentPalette = { h, s: Math.max(45, s) };
      applyPalette(currentPalette.h, currentPalette.s);
      cycleAnimFrame = requestAnimationFrame(tick);
    }
    cycleAnimFrame = requestAnimationFrame(tick);
  }

  function setCycleMode(mode) {
    cycleMode = mode;
    if (colorMode === "dynamic") {
      if (crossAnimFrame) cancelAnimationFrame(crossAnimFrame);
      if (cycleAnimFrame) { cancelAnimationFrame(cycleAnimFrame); cycleAnimFrame = null; }
      if (mode === "cycle" && lastPalettes.length > 1) startCycle(lastPalettes);
      else { currentPalette = { ...lastPalettes[0] }; applyPalette(lastPalettes[0].h, Math.max(45, lastPalettes[0].s)); }
    }
    saveConfig();
  }

  // function setCrossfade(val) { // crossfade disabled
  //   crossfade = val;
  //   invoke("set_crossfade", { duration: val });
  //   saveConfig();
  // }

  // ── Config persistence ────────────────────────────────────────────
  function loadConfig() {
    try {
      const raw = localStorage.getItem("ytune-config");
      if (!raw) return;
      const cfg  = JSON.parse(raw);
      colorMode         = cfg.colorMode         || "dynamic";
      fixedTheme        = cfg.fixedTheme        ?? 0;
      vizColorMode      = cfg.vizColorMode      || "dynamic";
      cycleMode         = cfg.cycleMode         || "none";
      // crossfade      = cfg.crossfade         ?? 0; // crossfade disabled
      // migrate old single bgMode field
      if (cfg.bgMode && !cfg.bgBase) {
        if      (cfg.bgMode === "solid")    { bgBase = "solid"; bgViz = "none"; }
        else if (cfg.bgMode === "art")      { bgBase = "art";   bgViz = "none"; }
        else if (cfg.bgMode === "cava")     { bgBase = "art";   bgViz = "cava"; }
        else if (cfg.bgMode === "spectrum") { bgBase = "art";   bgViz = "spectrum"; }
      } else {
        bgBase = cfg.bgBase || "art";
        bgViz  = cfg.bgViz  || "none";
      }
    } catch {}
  }

  async function toggleDiscord() {
    discordEnabled = !discordEnabled;
    await invoke("discord_set", { enabled: discordEnabled }).catch(() => {});
  }

  function saveConfig() {
    try {
      localStorage.setItem("ytune-config", JSON.stringify({
        colorMode, fixedTheme, bgBase, bgViz, vizColorMode, cycleMode,
      }));
    } catch {}
  }

  // Heights for config panel resize
  const CFG_BASE   = 118; // header + padding + color-mode section
  const CFG_PRESET = 80;  // preset theme grid (only when colorMode=fixed)
  const CFG_BG     = 76;  // background section
  const CFG_VIZ    = 76;  // bar color section (only when bgMode=cava|spectrum)

  const CFG_VIZ_OPT   = 76; // visualizer row (always shown in config)
  const CFG_SMOOTH    = 76; // smooth transitions toggle (only when colorMode=dynamic)
  const CFG_CROSSFADE = 76; // discord presence row

  function syncConfigSize() {
    if (!showConfig) { syncSize(); return; }
    let h = CFG_BASE + CFG_BG + CFG_VIZ_OPT + CFG_CROSSFADE;
    if (colorMode === "fixed")                           h += CFG_PRESET;
    if (colorMode === "dynamic")                         h += CFG_SMOOTH;
    if (bgViz === "cava" || bgViz === "spectrum")        h += CFG_VIZ;
    invoke("resize_popup", { height: h });
  }

  function setColorMode(mode) {
    colorMode = mode;
    if (crossAnimFrame) cancelAnimationFrame(crossAnimFrame);
    if (cycleAnimFrame) { cancelAnimationFrame(cycleAnimFrame); cycleAnimFrame = null; }
    const h = mode === "fixed" ? THEMES[fixedTheme].h : lastPalettes[0].h;
    const s = mode === "fixed" ? THEMES[fixedTheme].s : lastPalettes[0].s;
    currentPalette = { h, s: Math.max(45, s) };
    applyPalette(h, s);
    if (mode === "dynamic" && cycleMode === "cycle" && lastPalettes.length > 1) startCycle(lastPalettes);
    syncConfigSize();
    saveConfig();
  }

  function selectTheme(index) {
    fixedTheme = index;
    colorMode  = "fixed";
    if (crossAnimFrame) cancelAnimationFrame(crossAnimFrame);
    if (cycleAnimFrame) { cancelAnimationFrame(cycleAnimFrame); cycleAnimFrame = null; }
    const t = THEMES[index];
    currentPalette = { h: t.h, s: Math.max(45, t.s) };
    applyPalette(t.h, t.s);
    syncConfigSize();
    saveConfig();
  }

  function setBgBase(base) {
    bgBase = base;
    saveConfig();
  }

  function setBgViz(viz) {
    bgViz = viz;
    if (viz === "cava" || viz === "spectrum") startViz(); else stopViz();
    syncConfigSize();
    saveConfig();
  }

  function setVizColorMode(mode) {
    vizColorMode = mode;
    saveConfig();
  }

  function openConfig() {
    showConfig = true;
    showVolume = false;
    showQueue  = false;
    syncConfigSize();
  }
  function closeConfig() { showConfig = false; syncSize(); }

  // ── Visualizer ────────────────────────────────────────────────────
  function vizColor(alpha = 1) {
    const p = vizColorMode === "dynamic" ? currentPalette : THEMES[fixedTheme];
    return `hsla(${p.h},${Math.max(45, p.s)}%,62%,${alpha})`;
  }

  function drawBars(ctx, w, h) {
    ctx.clearRect(0, 0, w, h);
    const half = Math.floor(VIZ_N / 2); // 12
    const barW = w / VIZ_N;
    for (let i = 0; i < VIZ_N; i++) {
      // Mirror: left side reversed (treble→bass), right side normal (bass→treble)
      const barIdx = i < half ? (half - 1 - i) : (i - half);
      const v = vizBars[barIdx];
      const x = i * barW + 1;
      if (bgViz === "cava") {
        const bh = v * h * 0.85;
        const g = ctx.createLinearGradient(0, h, 0, h - bh);
        g.addColorStop(0, vizColor(0.75));
        g.addColorStop(1, vizColor(0.15));
        ctx.fillStyle = g;
        ctx.fillRect(x, h - bh, barW - 2, bh);
      } else {
        const bh = v * h * 0.45;
        const g = ctx.createLinearGradient(0, h / 2 - bh, 0, h / 2 + bh);
        g.addColorStop(0,   vizColor(0.10));
        g.addColorStop(0.5, vizColor(0.75));
        g.addColorStop(1,   vizColor(0.10));
        ctx.fillStyle = g;
        ctx.fillRect(x, h / 2 - bh, barW - 2, bh * 2);
      }
    }
  }

  function vizTick() {
    try {
      vizPhase += 0.025;
      const hasRealData = rawBars.some(v => v > 0);
      if (hasRealData) {
        for (let i = 0; i < VIZ_N; i++) {
          const srcI = Math.floor(i * rawBars.length / VIZ_N);
          vizTargets[i] = playing ? rawBars[srcI] / 255 : 0;
        }
      } else {
        for (let i = 0; i < VIZ_N; i++) {
          if (!playing) { vizTargets[i] = 0.02; continue; }
          const base  = Math.max(0, 1 - i / VIZ_N * 0.55) * 0.35;
          const noise = Math.sin(vizPhase * 2.1 + i * 0.6) * 0.28
                      + Math.sin(vizPhase * 0.9 + i * 1.1) * 0.18
                      + Math.sin(vizPhase * 3.5 + i * 0.4) * 0.09;
          vizTargets[i] = Math.max(0.05, Math.min(1, base + noise + 0.22));
        }
      }
      for (let i = 0; i < VIZ_N; i++) {
        const spd = vizTargets[i] > vizBars[i] ? 0.40 : 0.08;
        vizBars[i] += (vizTargets[i] - vizBars[i]) * spd;
      }
      if (vizCanvas && (bgViz === "cava" || bgViz === "spectrum")) {
        drawBars(vizCanvas.getContext("2d"), vizCanvas.width, vizCanvas.height);
      }
    } catch(e) {
    }
    vizFrame = requestAnimationFrame(vizTick);
  }

  function startViz() {
    if (vizFrame) return;
    vizFrame = requestAnimationFrame(vizTick);
  }

  function stopViz() {
    if (vizFrame) { cancelAnimationFrame(vizFrame); vizFrame = null; }
    vizBars    = Array(VIZ_N).fill(0);
    vizTargets = Array(VIZ_N).fill(0);
    if (vizCanvas) vizCanvas.getContext("2d")?.clearRect(0, 0, vizCanvas.width, vizCanvas.height);
  }

  // ── Event listener ────────────────────────────────────────────────
  onMount(async () => {
    await tick();

    vizCanvas = document.querySelector('canvas') ?? document.querySelector('.viz-canvas');
    loadConfig();
    discordEnabled = await invoke("discord_get").catch(() => true);
    const initH = colorMode === "fixed" ? THEMES[fixedTheme].h : 280;
    const initS = colorMode === "fixed" ? THEMES[fixedTheme].s : 65;
    currentPalette = { h: initH, s: initS };
    applyPalette(initH, initS);
    // invoke("set_crossfade", { duration: crossfade }); // crossfade disabled
    if (bgViz === "cava" || bgViz === "spectrum") startViz();
    unlistenViz = await listen("player-viz", (e) => {
      try {
        const arr = typeof e.payload === 'string' ? JSON.parse(e.payload) : e.payload;
        if (Array.isArray(arr)) rawBars = arr;
      } catch {}
    });
    unlisten = await listen("player_state_changed", (e) => {
      const p = e.payload;
      const newTitle = p.title    || "Nothing playing";
      const newDur   = p.duration || 0;
      const newTime  = p.currentTime || 0;

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
        // Keep liked/disliked false until next poll confirms new song's state
        liked    = false;
        disliked = false;
      } else if (isSeeking && Math.abs(newTime - seekValue) < 3) {
        // currentTime arrived close to where we seeked — release the bar
        isSeeking = false;
        clearTimeout(seekTimeout);
        currentTime = newTime;
        seekValue   = newTime;
      } else if (!isSeeking) {
        currentTime = newTime;
        seekValue   = newTime;
        // Only update like state on stable polls (not right when song changes)
        liked    = p.liked    ?? false;
        disliked = p.disliked ?? false;
      }

      // Palette pre-computed by injection script (~1s after song change)
      const newPalettes = Array.isArray(p.paletteH)
        ? p.paletteH.map((h, i) => ({ h, s: p.paletteS?.[i] ?? 65 }))
        : p.paletteH !== undefined
          ? [{ h: p.paletteH, s: p.paletteS ?? 65 }]
          : [{ h: 280, s: 65 }];
      if (songChanged || newPalettes[0].h !== lastPalettes[0].h) {
        lastPalettes = newPalettes;
        if (colorMode === "dynamic") {
          smoothToColor(newPalettes[0].h, newPalettes[0].s, () => {
            if (cycleMode === "cycle" && newPalettes.length > 1) startCycle(newPalettes);
          });
        }
      }

      title     = newTitle;
      artist    = p.artist    || "";
      thumbnail = p.thumbnail || "";
      // Use data URI when available; clears on song change, repopulates ~1s later
      if (songChanged) thumbnailData = "";
      if (p.thumbnailData) thumbnailData = p.thumbnailData;
      playing   = p.playing;
      if (!isVolAdjusting) volume = p.volume ?? volume;
      if (!songChanged) duration = newDur;
      queue = p.queue || [];
    });
  });

  onDestroy(() => {
    if (crossAnimFrame) cancelAnimationFrame(crossAnimFrame);
    if (cycleAnimFrame) cancelAnimationFrame(cycleAnimFrame);
    unlisten?.();
    unlistenViz?.();
    stopViz();
    clearTimeout(seekTimeout);
    clearTimeout(volTimeout);
  });

  function fmt(secs) {
    if (!secs || isNaN(secs)) return "0:00";
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60).toString().padStart(2, "0");
    return `${m}:${s}`;
  }

  const control = (action) => invoke("player_control", { action });
  const openApp = () => { invoke("show_main_window"); invoke("hide_tray_popup"); };
  const close   = () => invoke("hide_tray_popup");

  function onHeaderDrag(e) {
    if (e.target.closest('button')) return;
    getCurrentWindow().startDragging();
  }

  function onSeekInput(e) { isSeeking = true; seekValue = +e.target.value; }
  function onSeekCommit(e) {
    seekValue = +e.target.value;
    invoke("player_seek", { position: seekValue });
    clearTimeout(seekTimeout);
    // Fallback: release after 5s if the player never confirms the new position
    seekTimeout = setTimeout(() => { isSeeking = false; }, 5000);
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

  const BASE_H  = 205;
  const VOL_H   = 24;
  const QUEUE_H = 210;

  function syncSize() {
    invoke("resize_popup", { height: BASE_H + (showVolume ? VOL_H : 0) + (showQueue ? QUEUE_H : 0) });
  }
  function toggleVolume() { showVolume = !showVolume; syncSize(); }
  function toggleQueue()  { showQueue  = !showQueue;  syncSize(); }

  const progressPct = $derived(
    duration > 0 ? ((isSeeking ? seekValue : currentTime) / duration) * 100 : 0
  );
</script>

<main data-tauri-drag-region>

  <!-- Blurred album art background — only when bgBase is "art" -->
  {#if bgBase === "art" && (thumbnailData || thumbnail)}
    <div class="bg-art" style={`background-image:url("${thumbnailData || thumbnail}")`} aria-hidden="true"></div>
  {/if}

  <!-- Visualizer canvas (active when bgViz is cava/spectrum) -->
  <canvas width="340" height="500" class="viz-canvas" class:viz-on={bgViz === "cava" || bgViz === "spectrum"}></canvas>

  <!-- ── Header ── -->
  <header data-tauri-drag-region onmousedown={onHeaderDrag}>
    <span class="brand">ytune</span>
    <div class="header-actions">
      {#if showConfig}
        <!-- Back arrow replaces the gear when in config mode -->
        <button onclick={closeConfig} aria-label="Back" title="Back">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="15 18 9 12 15 6"/>
          </svg>
        </button>
      {:else}
        <button onclick={openConfig} aria-label="Settings" title="Settings">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="3"/>
            <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/>
          </svg>
        </button>
      {/if}
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

  <!-- ── Config panel (replaces all other content) ── -->
  {#if showConfig}
    <section class="config-panel">
      <div class="cfg-section">
        <p class="cfg-label">Color mode</p>
        <div class="mode-row">
          <button
            class="mode-btn"
            class:mode-active={colorMode === "dynamic"}
            onclick={() => setColorMode("dynamic")}
          >Dynamic</button>
          <button
            class="mode-btn"
            class:mode-active={colorMode === "fixed"}
            onclick={() => setColorMode("fixed")}
          >Fixed</button>
        </div>
      </div>

      {#if colorMode === "dynamic"}
        <div class="cfg-section">
          <p class="cfg-label">Transition</p>
          <div class="mode-row">
            <button class="mode-btn" class:mode-active={cycleMode === "none"}  onclick={() => setCycleMode("none")}>None</button>
            <button class="mode-btn" class:mode-active={cycleMode === "cycle"} onclick={() => setCycleMode("cycle")}>Cycle</button>
          </div>
        </div>
      {/if}

      {#if colorMode === "fixed"}
        <div class="cfg-section">
          <p class="cfg-label">Preset</p>
          <div class="theme-grid">
            {#each THEMES as theme, i}
              <button
                class="theme-swatch"
                class:swatch-active={fixedTheme === i}
                style="background:hsl({theme.h},{theme.s}%,52%)"
                onclick={() => selectTheme(i)}
                title={theme.name}
              ></button>
            {/each}
          </div>
        </div>
      {/if}

      <div class="cfg-section">
        <p class="cfg-label">Background</p>
        <div class="mode-row">
          <button class="mode-btn" class:mode-active={bgBase === "solid"} onclick={() => setBgBase("solid")}>Solid</button>
          <button class="mode-btn" class:mode-active={bgBase === "art"}   onclick={() => setBgBase("art")}  >Art</button>
        </div>
      </div>

      <div class="cfg-section">
        <p class="cfg-label">Visualizer</p>
        <div class="mode-row">
          <button class="mode-btn" class:mode-active={bgViz === "none"}     onclick={() => setBgViz("none")}    >None</button>
          <button class="mode-btn" class:mode-active={bgViz === "cava"}     onclick={() => setBgViz("cava")}    >Cava</button>
          <button class="mode-btn" class:mode-active={bgViz === "spectrum"} onclick={() => setBgViz("spectrum")}>Spectrum</button>
        </div>
      </div>

      <!-- crossfade disabled
      <div class="cfg-section">
        <p class="cfg-label">Crossfade</p>
        <div class="mode-row">
          <button class="mode-btn" onclick={() => setCrossfade(0)}>Off</button>
          <button class="mode-btn" onclick={() => setCrossfade(3)}>3s</button>
          <button class="mode-btn" onclick={() => setCrossfade(5)}>5s</button>
        </div>
      </div>
      -->

      {#if bgViz === "cava" || bgViz === "spectrum"}
        <div class="cfg-section">
          <p class="cfg-label">Bar color</p>
          <div class="mode-row">
            <button class="mode-btn" class:mode-active={vizColorMode === "dynamic"} onclick={() => setVizColorMode("dynamic")}>Dynamic</button>
            <button class="mode-btn" class:mode-active={vizColorMode === "fixed"}   onclick={() => setVizColorMode("fixed")}  >Fixed</button>
          </div>
        </div>
      {/if}

      <div class="cfg-section">
        <p class="cfg-label">Discord Presence</p>
        <div class="mode-row">
          <button class="mode-btn" class:mode-active={discordEnabled}  onclick={toggleDiscord}>On</button>
          <button class="mode-btn" class:mode-active={!discordEnabled} onclick={toggleDiscord}>Off</button>
        </div>
      </div>

    </section>
  {:else}

  <!-- ── Song info ── -->
  <section class="info">
    {#if thumbnailData || thumbnail}
      <img class="thumb" src={thumbnailData || thumbnail} alt="" draggable="false" />
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
      <button class="icon-btn" class:active={liked}    onclick={() => control("like")}    aria-label="Like"    title="Like">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
          <path d="M1 21h4V9H1v12zm22-11c0-1.1-.9-2-2-2h-6.31l.95-4.57.03-.32c0-.41-.17-.79-.44-1.06L14.17 1 7.59 7.59C7.22 7.95 7 8.45 7 9v10c0 1.1.9 2 2 2h9c.83 0 1.54-.5 1.84-1.22l3.02-7.05c.09-.23.14-.47.14-.73v-2z"/>
        </svg>
      </button>
      <button class="icon-btn" class:active={disliked} onclick={() => control("dislike")} aria-label="Dislike" title="Dislike">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
          <path d="M15 3H6c-.83 0-1.54.5-1.84 1.22l-3.02 7.05c-.09.23-.14.47-.14.73v2c0 1.1.9 2 2 2h6.31l-.95 4.57-.03.32c0 .41.17.79.44 1.06L9.83 23l6.59-6.59c.36-.36.58-.86.58-1.41V5c0-1.1-.9-2-2-2zm4 0v12h4V3h-4z"/>
        </svg>
      </button>
      <button class="icon-btn" class:active={showVolume} onclick={toggleVolume} aria-label="Volume" title="Volume">
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
        <button class="icon-btn queue-toggle" class:active={showQueue} onclick={toggleQueue} aria-label="Queue" title="Queue">
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
      <button class="vol-mute" onclick={toggleMute} aria-label={volume === 0 ? "Unmute" : "Mute"}>
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
        min="0" max="100" step="1"
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
          <li class:current={item.current} class:q-clickable={!item.current} onclick={() => { if (!item.current) control(`queue_jump_${item.domIndex}`); }}>
            <span class="dot">{item.current ? "▶" : "·"}</span>
            <span class="q-title" title={item.title}>{item.title}</span>
            <span class="q-artist" title={item.artist}>{item.artist}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {/if} <!-- end {#if showConfig} else -->


</main>

<style>
  :global(*) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(html), :global(body) {
    background: transparent !important;
    overflow: hidden;
    height: 100%;
    width: 100%;
  }

  /* Dynamic palette — defaults to purple, updated by extractAndApply() */
  :global(:root) {
    --h: 280;
    --s: 65%;
    --accent:     hsl(var(--h), var(--s), 62%);
    --accent2:    hsl(calc(var(--h) + 35), var(--s), 62%);
    --accent-dim: hsla(var(--h), var(--s), 45%, 0.22);
    --grad:       linear-gradient(135deg, var(--accent), var(--accent2));
  }

  main {
    position: relative;
    z-index: 0;
    width: 100%;
    min-height: 100%;
    background: #1c1c1e;
    color: #f5f5f7;
    border-radius: 14px;
    padding: 13px 15px 12px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    font-family: -apple-system, "Segoe UI", system-ui, sans-serif;
    user-select: none;
    box-shadow: 0 12px 40px rgba(0,0,0,0.6);
    overflow: hidden;
  }

  /* Album art as blurred background layer (behind the canvas) */
  .bg-art {
    position: absolute;
    inset: -20px;
    background-size: cover;
    background-position: center;
    filter: blur(22px) brightness(0.18) saturate(2.5);
    z-index: -2;
    pointer-events: none;
  }

  /* Header */
  header { display: flex; align-items: center; justify-content: space-between; }
  .brand {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    background: var(--grad);
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    background-clip: text;
    transition: background 0.6s ease;
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
  .info { display: flex; align-items: center; gap: 10px; overflow: hidden; }
  .thumb {
    width: 52px; height: 52px; border-radius: 7px;
    object-fit: cover; flex-shrink: 0;
    box-shadow: 0 2px 12px rgba(0,0,0,0.5);
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
    -webkit-appearance: none; appearance: none;
    width: 100%; height: 3px; border-radius: 3px;
    outline: none; cursor: pointer; border: none; padding: 0; margin: 0;
    background: linear-gradient(
      to right,
      var(--accent)  0%,
      var(--accent2) var(--pct),
      rgba(255,255,255,0.12) var(--pct),
      rgba(255,255,255,0.12) 100%
    );
    transition: background 0.6s ease;
  }
  input[type=range]::-webkit-slider-runnable-track {
    height: 3px; border-radius: 3px; background: transparent;
  }
  input[type=range]::-webkit-slider-thumb {
    -webkit-appearance: none; appearance: none;
    width: 11px; height: 11px; border-radius: 50%;
    background: #fff; box-shadow: 0 0 4px rgba(0,0,0,0.4);
    transition: transform 0.1s; margin-top: -4px;
  }
  input[type=range]:hover::-webkit-slider-thumb { transform: scale(1.3); }

  .times { display: flex; justify-content: space-between; font-size: 10px; color: #636366; }

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
    background: var(--accent-dim);
    padding: 9px; margin: 0 3px; border-radius: 50%;
    transition: background 0.6s ease;
  }
  .play:hover { background: hsla(var(--h), var(--s), 45%, 0.38); }

  .secondary-controls { margin-left: auto; display: flex; align-items: center; }
  .icon-btn { color: #636366; padding: 6px; }
  .icon-btn:hover { color: #f5f5f7; background: none; }
  .icon-btn.active { color: var(--accent); transition: color 0.6s ease; }

  .queue-toggle { display: flex; align-items: center; gap: 4px; }
  .queue-count  { font-size: 10px; font-weight: 600; }

  /* Volume */
  .volume-section { display: flex; align-items: center; gap: 7px; padding: 0 1px; }
  .vol-mute {
    color: #636366; padding: 3px; flex-shrink: 0;
    background: none; border: none; cursor: pointer;
    display: flex; align-items: center; border-radius: 4px; transition: color 0.15s;
  }
  .vol-mute:hover { color: #f5f5f7; }
  .vol-pct { font-size: 10px; color: #636366; width: 28px; text-align: right; flex-shrink: 0; }

  /* Queue */
  .queue { border-top: 1px solid rgba(255,255,255,0.06); padding-top: 8px; }
  .queue-label {
    font-size: 10px; font-weight: 600; text-transform: uppercase;
    letter-spacing: 0.08em; color: #636366; margin-bottom: 5px;
  }
  ul { list-style: none; display: flex; flex-direction: column; gap: 1px; }
  li {
    display: grid; grid-template-columns: 14px 1fr auto;
    align-items: center; gap: 6px;
    padding: 6px 6px 6px 4px; border-radius: 6px; transition: background 0.12s;
  }
  li:hover          { background: rgba(255,255,255,0.05); }
  li.q-clickable    { cursor: pointer; }
  li.current        { background: var(--accent-dim); transition: background 0.6s ease; }

  .dot { font-size: 10px; color: #636366; text-align: center; line-height: 1; }
  li.current .dot { color: var(--accent); }

  .q-title {
    font-size: 12px; font-weight: 500;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  li.current .q-title { color: var(--accent2); transition: color 0.6s ease; }

  .q-artist {
    font-size: 11px; color: #636366;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    max-width: 90px; text-align: right;
  }

  /* ── Visualizer canvas ── */
  .viz-canvas {
    position: absolute;
    inset: 0;
    border-radius: 14px;
    z-index: -1;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.4s ease;
    width: 100%;
    height: 100%;
  }
  .viz-canvas.viz-on { opacity: 1; }

  /* ── Config panel ── */
  .config-panel {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 6px 0 4px;
    flex: 1;
  }
  .cfg-section { display: flex; flex-direction: column; gap: 8px; }
  .cfg-label {
    font-size: 10px; font-weight: 700;
    text-transform: uppercase; letter-spacing: 0.1em;
    color: #636366;
  }

  .mode-row { display: flex; gap: 6px; }
  .mode-btn {
    flex: 1; padding: 8px 2px;
    border-radius: 9px; border: 1px solid rgba(255,255,255,0.1);
    background: rgba(0,0,0,0.45);
    color: #d0d0d5; font-size: 11.5px; font-weight: 500;
    cursor: pointer; transition: background 0.15s, color 0.15s, border-color 0.15s;
    white-space: nowrap;
  }
  .mode-btn:hover { background: rgba(0,0,0,0.6); color: #f5f5f7; }
  .mode-btn.mode-active {
    background: var(--accent-dim);
    color: var(--accent);
    border-color: rgba(255,255,255,0.15);
  }

  .theme-grid { display: flex; gap: 8px; flex-wrap: wrap; }
  .theme-swatch {
    width: 30px; height: 30px;
    border-radius: 50%;
    border: 2px solid transparent;
    padding: 0; cursor: pointer;
    transition: transform 0.12s, border-color 0.15s, box-shadow 0.15s;
    outline: none;
  }
  .theme-swatch:hover { transform: scale(1.18); }
  .theme-swatch.swatch-active {
    border-color: #fff;
    box-shadow: 0 0 0 2px rgba(255,255,255,0.18);
  }


</style>
