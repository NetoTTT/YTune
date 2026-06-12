<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, onDestroy, tick } from "svelte";
  import { check } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { sendNotification } from "@tauri-apps/plugin-notification";
  import { emit } from "@tauri-apps/api/event";

  const MAX_ITEMS = 13;

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
  let disliked      = $state(false);
  let shuffled      = $state(false);
  let repeatMode    = $state('none'); // 'none' | 'all' | 'one'
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
  let queueThumbs     = $state(false);
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
  let showLinkInput  = $state(false);
  let linkUrl        = $state('');
  let clipboardUrl   = $state('');
  let clipboardPollInterval = null;
  let isSeeking      = $state(false);
  let seekValue      = $state(0);
  let isVolAdjusting = false;
  let seekTimeout;
  let volTimeout;
  let unlisten;
  let unlistenViz;
  let unlistenNav;
  let updateAvailable   = $state(null); // { version, update } or null
  let updateInstalling  = $state(false);
  let unlistenInstall;

  // ── Sync rooms ────────────────────────────────────────────────────
  const BACKEND_WS = 'wss://ytune.asktome.com.br/ws';
  let roomId           = $state(null);
  let roomRole         = $state(null);   // 'host' | 'member' | null
  let memberCount      = $state(0);
  let participants     = $state([]);     // [{role, username, avatarUrl}]
  let showRoomInput    = $state(false);
  let roomInputId      = $state('');
  let trackUrl         = $state('');
  let ws               = null;
  let wsReconnectTimer = null;
  let pendingSeekPos      = null;
  let _pendingStartedAtMs = null; // wall clock anchor for fresh seek when player loads
  let _lastPlayingSync    = null;
  let _playPauseAt        = 0;    // epoch ms of last play/pause toggle — debounce
  let _syncedToVideoId    = null;
  let _lastSentUrl        = '';
  let _lastSentPlaying    = null;
  let _lastSentPosition   = 0;
  const SYNC_DRIFT_MAX    = 8;
  const PLAY_PAUSE_GAP    = 1500; // ms — ignore play/pause messages within this window after toggling

  function extractVideoId(url) {
    try { const m = url.match(/[?&]v=([^&]+)/); return m ? m[1] : url; } catch { return url; }
  }

  function applyRoomMsg(msg) {
    roomId       = msg.roomId;
    roomRole     = msg.role;
    memberCount  = msg.memberCount ?? 0;
    participants = msg.participants ?? [];
    emit('ytune-room-status', msg);
  }

  // ── Visualizer canvas ─────────────────────────────────────────────
  let vizCanvas = null; // set in onMount via querySelector — bind:this unreliable in runes mode
  let vizFrame;
  const VIZ_MAX  = 64; // internal resolution — never changes
  let vizBars    = Array(VIZ_MAX).fill(0);
  let vizTargets = Array(VIZ_MAX).fill(0);
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
      queueThumbs = cfg.queueThumbs ?? false;
    } catch {}
  }

  async function toggleDiscord() {
    discordEnabled = !discordEnabled;
    await invoke("discord_set", { enabled: discordEnabled }).catch(() => {});
  }

  function saveConfig() {
    try {
      localStorage.setItem("ytune-config", JSON.stringify({
        colorMode, fixedTheme, bgBase, bgViz, vizColorMode, cycleMode, queueThumbs,
      }));
    } catch {}
  }

  // Heights for config panel resize (absolute — config replaces all content)
  const CFG_BASE      = 118;
  const CFG_PRESET    = 80;
  const CFG_BG        = 76;
  const CFG_VIZ       = 76;
  const CFG_VIZ_OPT   = 76;
  const CFG_SMOOTH    = 76;
  const CFG_CROSSFADE = 76;

  function syncConfigSize() {
    let h = CFG_BASE + CFG_BG + CFG_VIZ_OPT + CFG_CROSSFADE + 76; // 76 = Queue thumbnails (always visible)
    if (colorMode === "fixed")                    h += CFG_PRESET;
    if (colorMode === "dynamic")                  h += CFG_SMOOTH;
    if (bgViz === "cava" || bgViz === "spectrum") h += CFG_VIZ;
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

  let preConfigH = 225;
  let preConfigW = 330;

  async function openConfig() {
    preConfigH = window.innerHeight
      - (showVolume ? VOL_H : 0)
      - (showQueue  ? QUEUE_H : 0);
    preConfigW = window.innerWidth;
    showVolume = false;
    showQueue  = false;
    await tick(); // let DOM settle before swapping content
    syncConfigSize(); // start window resize first
    await new Promise(r => setTimeout(r, 30)); // let resize start before content appears
    showConfig = true;
  }
  async function closeConfig() {
    showConfig = false;
    await tick(); // let music content render before shrinking
    invoke("set_popup_size", { width: preConfigW, height: preConfigH });
  }

  // ── Visualizer ────────────────────────────────────────────────────
  function vizColor(alpha = 1) {
    const p = vizColorMode === "dynamic" ? currentPalette : THEMES[fixedTheme];
    return `hsla(${p.h},${Math.max(45, p.s)}%,62%,${alpha})`;
  }

  function drawBars(ctx, w, h) {
    ctx.clearRect(0, 0, w, h);
    const n    = Math.max(16, Math.min(VIZ_MAX, Math.round(w / 14)));
    const half = Math.floor(n / 2);
    const barW = w / n;
    for (let i = 0; i < n; i++) {
      const barIdx = i < half ? (half - 1 - i) : (i - half);
      // map drawn bar → internal array (proportional sampling)
      const srcI = Math.floor(barIdx * (VIZ_MAX / 2) / Math.max(1, half));
      const v = vizBars[Math.min(srcI, VIZ_MAX - 1)];
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
      if (document.hidden) { vizFrame = requestAnimationFrame(vizTick); return; }
      vizPhase += 0.025;
      const hasRealData = rawBars.some(v => v > 0);
      if (hasRealData) {
        for (let i = 0; i < VIZ_MAX; i++) {
          const srcI = Math.floor(i * rawBars.length / VIZ_MAX);
          vizTargets[i] = playing ? rawBars[srcI] / 255 : 0;
        }
      } else {
        for (let i = 0; i < VIZ_MAX; i++) {
          if (!playing) { vizTargets[i] = 0.02; continue; }
          const base  = Math.max(0, 1 - i / VIZ_MAX * 0.55) * 0.35;
          const noise = Math.sin(vizPhase * 2.1 + i * 0.6) * 0.28
                      + Math.sin(vizPhase * 0.9 + i * 1.1) * 0.18
                      + Math.sin(vizPhase * 3.5 + i * 0.4) * 0.09;
          vizTargets[i] = Math.max(0.05, Math.min(1, base + noise + 0.22));
        }
      }
      for (let i = 0; i < VIZ_MAX; i++) {
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
    vizBars    = Array(VIZ_MAX).fill(0);
    vizTargets = Array(VIZ_MAX).fill(0);
    if (vizCanvas) vizCanvas.getContext("2d")?.clearRect(0, 0, vizCanvas.width, vizCanvas.height);
  }

  // ── Event listener ────────────────────────────────────────────────
  onMount(async () => {
    await tick();

    vizCanvas = document.querySelector('canvas') ?? document.querySelector('.viz-canvas');
    if (vizCanvas) {
      vizCanvas.width  = window.innerWidth;
      vizCanvas.height = window.innerHeight;
    }
    loadConfig();
    discordEnabled = await invoke("discord_get").catch(() => true);
    const initH = colorMode === "fixed" ? THEMES[fixedTheme].h : 280;
    const initS = colorMode === "fixed" ? THEMES[fixedTheme].s : 65;
    currentPalette = { h: initH, s: initS };
    applyPalette(initH, initS);
    // invoke("set_crossfade", { duration: crossfade }); // crossfade disabled
    await restorePopupSize();
    window.addEventListener('resize', savePopupSize);
    window.addEventListener('focus', checkClipboard);
    window.addEventListener('keydown', (e) => {
      if (e.key === 'F12') invoke('open_devtools', { window: 'tray-popup' });
    });
    checkClipboard();
    checkForUpdate();
    invoke('get_auth_token').then(t => {
      console.log('[ytune-room] auth token:', t ? 'present' : 'null');
      if (t) connectWs(t); else wsStatus = 'no-token';
    }).catch(() => { wsStatus = 'no-token'; });
    // Also connect when token is restored from YTM localStorage (app restart case)
    listen('ytune-auth-ready', (e) => {
      const t = e.payload;
      if (t) connectWs(t);
    });
    if (bgViz === "cava" || bgViz === "spectrum") startViz();
    unlistenInstall = await listen("ytune-install-update", () => installUpdate());
    unlistenNav = await listen("ytune-navigating", () => {
      queue = [];
      showQueue = false;
      title = '';
      artist = '';
      thumbnail = '';
      thumbnailData = '';
    });
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
      if (p.trackUrl) trackUrl = p.trackUrl;
      // Use data URI when available; clears on song change, repopulates ~1s later
      if (songChanged) thumbnailData = "";
      if (p.thumbnailData) thumbnailData = p.thumbnailData;
      playing   = p.playing;
      // Sync room: consume pending seek once player has loaded the new song (duration > 0 = metadata ready)
      if (pendingSeekPos !== null && p.duration > 0) {
        // Recompute position from wall clock to correct for navigation delay
        const pos = _pendingStartedAtMs !== null
          ? Math.max(0, (Date.now() - _pendingStartedAtMs) / 1000)
          : pendingSeekPos;
        pendingSeekPos = null;
        _pendingStartedAtMs = null;
        console.log('[ytune-room] player ready after nav, seeking to', pos.toFixed(1));
        setTimeout(() => invoke('player_seek', { position: pos }).catch(() => {}), 300);
      }
      wsSendState();
      if (!isVolAdjusting) volume = p.volume ?? volume;
      if (!songChanged) duration = newDur;
      shuffled   = p.shuffled   ?? false;
      repeatMode = p.repeatMode ?? 'none';
      queue = p.queue || [];
    });
  });

  onDestroy(() => {
    window.removeEventListener('resize', savePopupSize);
    window.removeEventListener('focus', checkClipboard);
    clearInterval(clipboardPollInterval);
    if (crossAnimFrame) cancelAnimationFrame(crossAnimFrame);
    if (cycleAnimFrame) cancelAnimationFrame(cycleAnimFrame);
    unlisten?.();
    unlistenViz?.();
    unlistenInstall?.();
    unlistenNav?.();
    clearTimeout(wsReconnectTimer);
    ws?.close();
    stopViz();
    clearTimeout(seekTimeout);
    clearTimeout(volTimeout);
  });

  function fmt(secs) {
    if (!secs || isNaN(secs)) return "0:00";
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const s = Math.floor(secs % 60).toString().padStart(2, "0");
    if (h > 0) return `${h}:${m.toString().padStart(2, "0")}:${s}`;
    return `${m}:${s}`;
  }

  const control = (action) => invoke("player_control", { action });
  const openApp = () => { invoke("show_main_window"); invoke("hide_tray_popup"); };
  const close   = () => invoke("hide_tray_popup");

  async function checkForUpdate() {
    try {
      const update = await check();
      if (update?.available) {
        updateAvailable = { version: update.version, update };
        await emit('ytune-update-available', { version: update.version });
        sendNotification({ title: 'ytune', body: `Atualização ${update.version} disponível — abra o ytune para instalar.` });
      }
    } catch {}
  }

  async function installUpdate() {
    if (!updateAvailable || updateInstalling) return;
    updateInstalling = true;
    invoke('show_main_window');
    try {
      await updateAvailable.update.downloadAndInstall();
      await relaunch();
    } catch { updateInstalling = false; }
  }

  async function checkClipboard() {
    try {
      const text = (await invoke('read_clipboard')).trim();
      clipboardUrl = text.includes('music.youtube.com') ? text : '';
    } catch { clipboardUrl = ''; }
  }

  function toggleLinkInput() {
    showLinkInput = !showLinkInput;
    if (showLinkInput) {
      showRoomInput = false;
      linkUrl = clipboardUrl;
      clipboardPollInterval = setInterval(async () => {
        const prev = clipboardUrl;
        await checkClipboard();
        // Auto-update field only if user hasn't typed something else
        if (clipboardUrl !== prev && (linkUrl === prev || linkUrl === '')) {
          linkUrl = clipboardUrl;
        }
      }, 1000);
    } else {
      linkUrl = '';
      clearInterval(clipboardPollInterval);
      clipboardPollInterval = null;
    }
  }

  async function navigateUrl() {
    const url = linkUrl.trim();
    if (!url.includes('music.youtube.com')) return;
    try {
      await invoke('navigate_ytm', { url });
      showLinkInput = false;
      linkUrl = '';
      clipboardUrl = '';
      // Clear stale state immediately — inject will repopulate once new page loads
      queue = [];
      showQueue = false;
      title = '';
      artist = '';
      thumbnail = '';
      thumbnailData = '';
    } catch {}
  }

  function onLinkKeydown(e) {
    if (e.key === 'Enter') navigateUrl();
    if (e.key === 'Escape') { showLinkInput = false; linkUrl = ''; }
  }

  // ── WebSocket / room logic ────────────────────────────────────────
  let wsStatus = $state('disconnected'); // 'disconnected' | 'connecting' | 'connected' | 'no-token'

  function connectWs(token) {
    if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return;
    wsStatus = 'connecting';
    console.log('[ytune-room] connecting to', BACKEND_WS);
    ws = new WebSocket(`${BACKEND_WS}?token=${encodeURIComponent(token)}`);
    ws.onopen  = () => {
      wsStatus = 'connected';
      console.log('[ytune-room] WS connected');
      // If we were in a room as member, re-join automatically after reconnect
      if (roomId && roomRole === 'member') {
        console.log('[ytune-room] auto-rejoining room', roomId);
        ws.send(JSON.stringify({ type: 'join_room', roomId }));
      }
    };
    ws.onmessage = (e) => { try { handleWsMsg(JSON.parse(e.data)); } catch {} };
    ws.onerror = (e) => { console.error('[ytune-room] WS error', e); };
    ws.onclose   = (e) => {
      wsStatus = 'disconnected';
      console.log('[ytune-room] WS closed', e.code, e.reason);
      ws = null;
      clearTimeout(wsReconnectTimer);
      wsReconnectTimer = setTimeout(async () => {
        const t = await invoke('get_auth_token').catch(() => null);
        if (t) connectWs(t); else wsStatus = 'no-token';
      }, 5000);
    };
  }

  function handleWsMsg(msg) {
    console.log('[ytune-room] msg:', JSON.stringify(msg));
    switch (msg.type) {
      case 'room_created':
      case 'room_joined':
      case 'room_status':
        applyRoomMsg(msg);
        break;
      case 'member_joined':
      case 'member_left':
        memberCount  = msg.count;
        participants = msg.participants ?? participants;
        emit('ytune-room-status', { roomId, role: roomRole, memberCount, participants });
        break;
      case 'play_now': {
        if (roomRole !== 'member' || !msg.url) break;
        const hostVid = extractVideoId(msg.url);

        // Compute target position from wall clock
        const targetPos = msg.playing
          ? Math.max(0, (Date.now() - msg.startedAtMs) / 1000)
          : msg.pausedElapsedMs / 1000;

        if (_syncedToVideoId === hostVid) {
          if (pendingSeekPos !== null) {
            // Navigation still in progress — just update the seek target, never seek now
            pendingSeekPos = targetPos;
          } else {
            // Stable: check drift and play/pause
            const drift = Math.abs(targetPos - currentTime);
            if (drift > SYNC_DRIFT_MAX) {
              console.log('[ytune-room] play_now: drift', drift.toFixed(1), 's → seek to', targetPos.toFixed(1));
              invoke('player_seek', { position: targetPos }).catch(() => {});
            }
            if (typeof msg.playing === 'boolean' && msg.playing !== _lastPlayingSync && msg.playing !== playing
                && (Date.now() - _playPauseAt) > PLAY_PAUSE_GAP) {
              _lastPlayingSync = msg.playing;
              _playPauseAt = Date.now();
              invoke('player_control', { action: 'play_pause' }).catch(() => {});
            }
          }
        } else {
          // New song — navigate and seek once player is ready
          console.log('[ytune-room] play_now: navigate to', hostVid, 'seek=', targetPos.toFixed(1));
          _syncedToVideoId    = hostVid;
          _pendingStartedAtMs = msg.playing ? msg.startedAtMs : null;
          if (pendingSeekPos !== null) {
            pendingSeekPos = targetPos;
          } else {
            pendingSeekPos = targetPos;
            setTimeout(() => { pendingSeekPos = null; _pendingStartedAtMs = null; }, 20000);
            invoke('navigate_ytm', { url: msg.url }).catch(() => { pendingSeekPos = null; _pendingStartedAtMs = null; });
          }
          // Don't toggle play/pause mid-navigation — let it stabilize first
        }
        break;
      }
      case 'paused': {
        if (roomRole !== 'member') break;
        if (playing && _lastPlayingSync !== false && (Date.now() - _playPauseAt) > PLAY_PAUSE_GAP) {
          _lastPlayingSync = false;
          _playPauseAt = Date.now();
          invoke('player_control', { action: 'play_pause' }).catch(() => {});
        }
        break;
      }
      case 'resumed': {
        if (roomRole !== 'member') break;
        if (!playing && _lastPlayingSync !== true && (Date.now() - _playPauseAt) > PLAY_PAUSE_GAP) {
          _lastPlayingSync = true;
          _playPauseAt = Date.now();
          invoke('player_control', { action: 'play_pause' }).catch(() => {});
        }
        if (msg.startedAtMs && (Date.now() - _playPauseAt) > PLAY_PAUSE_GAP) {
          const pos = Math.max(0, (Date.now() - msg.startedAtMs) / 1000);
          const drift = Math.abs(pos - currentTime);
          if (drift > SYNC_DRIFT_MAX) {
            invoke('player_seek', { position: pos }).catch(() => {});
          }
        }
        break;
      }
      case 'room_dissolved':
      case 'room_left':
        roomId = null; roomRole = null; memberCount = 0;
        _syncedToVideoId = null; _lastPlayingSync = null; _pendingStartedAtMs = null;
        _lastSentUrl = ''; _lastSentPlaying = null; _lastSentPosition = 0; _playPauseAt = 0;
        emit('ytune-room-status', {});
        break;
    }
  }

  function wsSendState() {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    if (roomRole !== 'host') return;
    if (!trackUrl) return;

    // New or changed song → play_track with current position
    if (trackUrl !== _lastSentUrl) {
      _lastSentUrl      = trackUrl;
      _lastSentPlaying  = playing;
      _lastSentPosition = currentTime;
      ws.send(JSON.stringify({ type: 'play_track', url: trackUrl, position: Math.round(currentTime) }));
      return;
    }

    // Play/pause changed → pause or resume
    if (playing !== _lastSentPlaying) {
      _lastSentPlaying = playing;
      ws.send(JSON.stringify({ type: playing ? 'resume' : 'pause' }));
      _lastSentPosition = currentTime;
      return;
    }

    // Seek detection: position jumped more than expected natural ~1s advance
    const expectedDrift = currentTime - (_lastSentPosition + 1.5);
    _lastSentPosition = currentTime;
    if (Math.abs(expectedDrift) > 5) {
      console.log('[ytune-room] host seek detected, resending play_track at', currentTime.toFixed(1));
      ws.send(JSON.stringify({ type: 'play_track', url: trackUrl, position: Math.round(currentTime) }));
    }
  }

  function joinRoom() {
    const id = roomInputId.trim().toUpperCase();
    if (!id || !ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ type: 'join_room', roomId: id }));
    showRoomInput = false;
    roomInputId = '';
  }

  function leaveRoom() {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ type: 'leave_room' }));
  }

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

  function scrollOnHover(node) {
    function start() {
      const overflow = node.offsetWidth - wrap.clientWidth;
      if (overflow <= 2) return;
      node.style.setProperty('--marquee-d', `-${overflow + 8}px`);
      node.classList.add('scrolling');
    }
    function stop() {
      node.classList.remove('scrolling');
    }
    const wrap = node.parentElement;
    wrap.addEventListener('mouseenter', start);
    wrap.addEventListener('mouseleave', stop);
    return { destroy() { wrap.removeEventListener('mouseenter', start); wrap.removeEventListener('mouseleave', stop); } };
  }

  function toggleMute() {
    if (volume > 0) { prevVolume = volume; volume = 0; }
    else             { volume = prevVolume || 100; }
    invoke("player_volume", { volume });
  }

  const VOL_H     = 24;
  const QUEUE_H   = 210;
  const DEFAULT_W = 330;
  const DEFAULT_H = 225;

  function toggleVolume() {
    showVolume = !showVolume;
    invoke("resize_popup", { height: window.innerHeight + (showVolume ? VOL_H : -VOL_H) });
  }
  let preQueueH = 0;

  async function toggleQueue() {
    if (showQueue) {
      showQueue = false;
      invoke("resize_popup", { height: preQueueH });
    } else {
      preQueueH = window.innerHeight;
      showQueue = true;
      await tick();
      const qEl = document.querySelector('section.queue');
      if (qEl) {
        // clientHeight = actual rendered height of queue (flex:none, capped by max-height)
        // grow window by that amount so .info section isn't stolen from
        invoke("resize_popup", { height: window.innerHeight + qEl.clientHeight + 8 });
      }
    }
  }

  function savePopupSize() {
    if (showConfig) return;
    const h = window.innerHeight - (showVolume ? VOL_H : 0);
    try { localStorage.setItem('ytune-popup-size', JSON.stringify({ w: window.innerWidth, h })); } catch {}
    if (vizCanvas) {
      vizCanvas.width  = window.innerWidth;
      vizCanvas.height = window.innerHeight;
    }
  }

  async function restorePopupSize() {
    let w = DEFAULT_W, h = DEFAULT_H;
    try {
      const raw = localStorage.getItem('ytune-popup-size');
      if (raw) { const s = JSON.parse(raw); w = s.w || DEFAULT_W; h = s.h || DEFAULT_H; }
    } catch {}
    await invoke('set_popup_size', { width: w, height: h + (showVolume ? VOL_H : 0) });
  }

  const progressPct = $derived(
    duration > 0 ? ((isSeeking ? seekValue : currentTime) / duration) * 100 : 0
  );
</script>

<main data-tauri-drag-region class:queue-open={showQueue}>

  <!-- Blurred album art background — only when bgBase is "art" -->
  {#if bgBase === "art" && (thumbnailData || thumbnail)}
    <div class="bg-art" style={`background-image:url("${thumbnailData || thumbnail}")`} aria-hidden="true"></div>
  {/if}

  <!-- Visualizer canvas (active when bgViz is cava/spectrum) -->
  <canvas class="viz-canvas" class:viz-on={bgViz === "cava" || bgViz === "spectrum"}></canvas>

  <!-- ── Header ── -->
  <header data-tauri-drag-region onmousedown={onHeaderDrag}>
    <span class="brand">ytune</span>
    <div class="header-actions">
      {#if !showConfig}
        <button onclick={() => { showRoomInput = !showRoomInput; if (showRoomInput) showLinkInput = false; }} class:active-link={showRoomInput || roomId} aria-label="Sync room" title="Sync room">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/>
            <path d="M23 21v-2a4 4 0 00-3-3.87"/><path d="M16 3.13a4 4 0 010 7.75"/>
          </svg>
        </button>
        <button onclick={toggleLinkInput} class:active-link={showLinkInput} class:clip-ready={clipboardUrl && !showLinkInput} aria-label="Open URL" title="Open YouTube Music link">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71"/>
            <path d="M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71"/>
          </svg>
        </button>
      {/if}
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

    <!-- Transient input panels — overlaid below header, outside flex flow -->
    <div class="panel-overlay">
      {#if showRoomInput && !roomId}
        {#if wsStatus === 'no-token'}
          <div class="room-bar" style="background:rgba(255,69,58,0.12);border-color:rgba(255,69,58,0.3)">
            <span class="room-info"><span style="opacity:.7">Sign in to ytune to use rooms</span></span>
          </div>
        {:else if wsStatus !== 'connected'}
          <div class="room-bar" style="background:rgba(255,159,10,0.12);border-color:rgba(255,159,10,0.3)">
            <span class="room-info"><span style="opacity:.7">Connecting to server… ({wsStatus})</span></span>
          </div>
        {:else}
          <div class="link-bar">
            <input
              class="link-input"
              type="text"
              placeholder="Room ID (e.g. GUY4LX)"
              bind:value={roomInputId}
              onkeydown={(e) => { if (e.key === 'Enter') joinRoom(); if (e.key === 'Escape') showRoomInput = false; }}
              autofocus
            />
            <button class="link-go" onclick={joinRoom} disabled={roomInputId.trim().length < 4} aria-label="Join">
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                <line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/>
              </svg>
            </button>
          </div>
          <button class="add-bot-btn" onclick={() => emit('ytune-open-url', 'https://discord.com/oauth2/authorize?client_id=1513965769199980634')}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
              <path d="M20.317 4.37a19.791 19.791 0 00-4.885-1.515.074.074 0 00-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 00-5.487 0 12.64 12.64 0 00-.617-1.25.077.077 0 00-.079-.037A19.736 19.736 0 003.677 4.37a.07.07 0 00-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 00.031.057 19.9 19.9 0 005.993 3.03.078.078 0 00.084-.028 14.09 14.09 0 001.226-1.994.076.076 0 00-.041-.106 13.107 13.107 0 01-1.872-.892.077.077 0 01-.008-.128 10.2 10.2 0 00.372-.292.074.074 0 01.077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 01.078.01c.12.098.246.198.373.292a.077.077 0 01-.006.127 12.299 12.299 0 01-1.873.892.077.077 0 00-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 00.084.028 19.839 19.839 0 006.002-3.03.077.077 0 00.032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 00-.031-.03z"/>
            </svg>
            Add bot to Discord
          </button>
        {/if}
      {/if}
      {#if showLinkInput}
        <div class="link-bar">
          <input
            class="link-input"
            type="url"
            placeholder="Paste a YouTube Music link..."
            bind:value={linkUrl}
            onkeydown={onLinkKeydown}
            autofocus
          />
          <button class="link-go" onclick={navigateUrl} disabled={!linkUrl.includes('music.youtube.com')} aria-label="Open">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
              <line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/>
            </svg>
          </button>
        </div>
      {/if}
    </div>
  </header>

  <!-- ── Room status bar ── -->
  {#if roomId}
    <div class="room-bar">
      <span class="room-info">
        {#if participants.length > 0}
          <div class="room-avatars">
            {#each participants as p, i}
              <img class="room-avatar" src={p.avatarUrl} alt={p.username}
                title="{p.role === 'host' ? '👑 ' : ''}{p.username}"
                style="opacity:{Math.max(0.35, 1 - i * 0.2)}; margin-left:{i > 0 ? '-7px' : '0'}; z-index:{participants.length - i}" />
            {/each}
          </div>
        {:else}
          <span style="opacity:.5;font-size:10px">{roomRole === 'host' ? '📡' : '🎵'}</span>
        {/if}
        <strong>{roomId}</strong>
        <span class="room-sub">{roomRole === 'host' ? 'host' : 'listening'}{memberCount > 0 ? ` · ${memberCount}` : ''}</span>
      </span>
      <button class="room-leave" onclick={leaveRoom} title="Leave room">✕</button>
    </div>
  {/if}

  <!-- ── Update banner ── -->
  {#if updateAvailable}
    <div class="update-banner">
      <span>{updateInstalling ? 'Downloading update…' : `Update ${updateAvailable.version} available`}</span>
      <button class="update-btn" onclick={installUpdate} disabled={updateInstalling}>
        {updateInstalling ? '…' : 'Install'}
      </button>
    </div>
  {/if}

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
      <div class="cfg-section">
        <p class="cfg-label">Queue thumbnails</p>
        <div class="mode-row">
          <button class="mode-btn" class:mode-active={queueThumbs}  onclick={() => { queueThumbs = true;  saveConfig(); }}>On</button>
          <button class="mode-btn" class:mode-active={!queueThumbs} onclick={() => { queueThumbs = false; saveConfig(); }}>Off</button>
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
      <div class="scroll-clip"><span class="title" use:scrollOnHover>{title}</span></div>
      <div class="scroll-clip"><span class="artist" use:scrollOnHover>{artist}</span></div>
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

  <!-- ── Transport controls ── -->
  <section class="controls">
    <button class="icon-btn" class:active={shuffled} onclick={() => control("shuffle")} aria-label="Shuffle" title="Shuffle">
      <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
        <path d="M16.293 1.293a1 1 0 00-.001 1.415L18.585 5H17.21a7 7 0 00-5.823 3.118L6.95 14.774A5 5 0 012.79 17H2a1 1 0 000 2h.79a7 7 0 005.822-3.117l4.438-6.656A5 5 0 0117.21 7h1.376l-2.293 2.293a1 1 0 001.414 1.414L22.414 6l-4.707-4.707a1 1 0 00-1.414 0ZM2.789 5H2a1 1 0 000 2h.79a5 5 0 014.159 2.227l.647.97 1.202-1.802-.185-.277A7 7 0 002.789 5Zm13.504 8.293a1 1 0 00-.001 1.414L18.585 17H17.21a5 5 0 01-4.16-2.226l-.648-.972-1.202 1.803.186.278A7 7 0 0017.21 19h1.376l-2.293 2.294-.068.076a1 1 0 001.406 1.406l.076-.07L22.414 18l-4.707-4.707a1 1 0 00-1.414 0Z"/>
      </svg>
    </button>

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

    <button class="icon-btn repeat-btn" class:active={repeatMode !== 'none'} onclick={() => control("repeat")} aria-label="Repeat" title="Repeat">
      <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
        <path d="M17.293 1.293a1 1 0 000 1.415L18.586 4H7a5 5 0 00-5 5v4a1 1 0 102 0V9a3 3 0 013-3h11.586l-1.293 1.293a1 1 0 001.414 1.415L22.414 5l-3.707-3.707a1 1 0 00-1.414 0ZM21 10a1 1 0 00-1 1v4a3 3 0 01-3 3H5.414l1.293-1.292a1.001 1.001 0 00-1.414-1.415L1.586 19l3.707 3.707a1 1 0 101.414-1.413L5.414 20H17a5 5 0 005-5v-4a1 1 0 00-1-1Z"/>
      </svg>
      {#if repeatMode === 'one'}
        <span class="repeat-one">1</span>
      {/if}
    </button>
  </section>

  <!-- ── Action controls ── -->
  <section class="actions">
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

    <div class="actions-spacer"></div>

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
      <ul class:with-thumbs={queueThumbs}>
        {#each queue.slice(0, MAX_ITEMS) as item}
          <li class:current={item.current} class:q-clickable={!item.current} onclick={() => { if (!item.current) control(`queue_jump_${item.domIndex}`); }}>
            {#if queueThumbs && item.thumb}
              <img class="q-thumb" src={item.thumb} alt="" draggable="false" />
            {:else}
              <span class="dot" style={queueThumbs ? 'justify-self:center' : ''}>{item.current ? "▶" : "·"}</span>
            {/if}
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
    width: 100%; height: 100vh;
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
  header { display: flex; align-items: center; justify-content: space-between; position: relative; overflow: visible; }
  .panel-overlay {
    position: absolute;
    top: calc(100% + 9px);
    left: 0; right: 0;
    z-index: 20;
    display: flex; flex-direction: column; gap: 4px;
    pointer-events: none;
  }
  .panel-overlay > * { pointer-events: auto; }
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
  .header-actions .active-link { color: var(--accent); }
  .header-actions .clip-ready  { color: var(--accent); opacity: 0.7; }

  /* Room bar */
  .room-bar {
    display: flex; align-items: center; justify-content: space-between;
    background: var(--accent-dim);
    border-radius: 6px; border: 1px solid rgba(255,255,255,0.07);
    padding: 2px 8px 2px 6px; font-size: 10px; color: rgba(255,255,255,0.85);
    animation: panel-enter 80ms ease-out both;
    flex-shrink: 0;
  }
  .room-info { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .room-info strong { color: var(--accent); letter-spacing: 0.04em; flex-shrink: 0; }
  .room-sub { opacity: 0.5; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .room-leave {
    background: none; border: none; color: rgba(255,255,255,0.35);
    cursor: pointer; padding: 2px 2px 2px 6px; font-size: 10px; line-height: 1; flex-shrink: 0;
  }
  .room-leave:hover { color: #ff453a; }
  .add-bot-btn {
    display: flex; align-items: center; justify-content: center; gap: 5px;
    width: 100%; padding: 5px 10px;
    background: rgba(28,28,30,0.82); backdrop-filter: blur(14px);
    border: 1px solid rgba(88, 101, 242, 0.35);
    border-radius: 8px; color: rgba(255,255,255,0.55); font-size: 10px;
    cursor: pointer; transition: background 0.15s, color 0.15s;
  }
  .add-bot-btn:hover { background: rgba(88, 101, 242, 0.25); color: rgba(255,255,255,0.9); }
  .room-avatars { display: flex; align-items: center; }
  .room-avatar {
    width: 18px; height: 18px; border-radius: 50%;
    border: 1.5px solid rgba(0,0,0,0.5);
    object-fit: cover; flex-shrink: 0;
    position: relative;
  }

  /* Update banner */
  .update-banner {
    display: flex; align-items: center; justify-content: space-between;
    background: rgba(48, 209, 88, 0.12); border-radius: 8px;
    padding: 6px 10px; font-size: 11px; color: #30d158;
    animation: panel-enter 80ms ease-out both;
  }
  .update-btn {
    padding: 3px 10px; border-radius: 6px; font-size: 11px; font-weight: 600;
    background: #30d158; color: #000; transition: opacity 0.15s;
  }
  .update-btn:disabled { opacity: 0.5; pointer-events: none; }

  /* URL input bar */
  .link-bar {
    display: flex; align-items: center; gap: 6px;
    background: rgba(28,28,30,0.82);
    backdrop-filter: blur(14px);
    border: 1px solid rgba(255,255,255,0.07);
    border-radius: 8px; padding: 4px 6px 4px 10px;
    animation: panel-enter 80ms ease-out both;
  }
  .link-input {
    flex: 1; background: none; border: none; outline: none;
    color: #f5f5f7; font-size: 11px; font-family: inherit;
    padding: 0;
  }
  .link-input::placeholder { color: #636366; }
  .link-go {
    flex-shrink: 0; padding: 4px 6px; border-radius: 6px;
    color: var(--accent); transition: background 0.15s, opacity 0.15s;
  }
  .link-go:disabled { opacity: 0.25; pointer-events: none; }

  /* Song info — fluid layout */
  .info {
    display: flex; align-items: center; gap: 10px; overflow: hidden;
    flex: 1; min-height: 0;
    transition: flex-direction 0.3s;
  }
  .thumb {
    width: clamp(48px, 20vh, 120px);
    height: clamp(48px, 20vh, 120px);
    border-radius: clamp(7px, 2vh, 18px);
    object-fit: cover; flex-shrink: 0;
    box-shadow: 0 clamp(2px, 1vh, 8px) clamp(8px, 4vh, 24px) rgba(0,0,0,0.6);
    transition: width 0.4s ease, height 0.4s ease, border-radius 0.4s ease, box-shadow 0.4s ease;
  }
  .thumb-placeholder {
    background: rgba(255,255,255,0.06);
    display: flex; align-items: center; justify-content: center;
    color: #f5f5f7;
  }
  .info-text { flex: 1; overflow: hidden; }
  .scroll-clip { overflow: hidden; }
  .title {
    display: inline-block;
    font-size: clamp(13px, 2.2vh, 20px); font-weight: 600;
    white-space: nowrap; line-height: 1.3;
    transition: font-size 0.3s ease;
  }
  .artist {
    display: inline-block;
    font-size: clamp(11px, 1.7vh, 15px); color: #8e8e93; margin-top: 2px;
    white-space: nowrap;
    transition: font-size 0.3s ease;
  }

  /* Medium layout — left-hero: large thumb left, info right */
  @media (min-height: 300px) and (max-height: 410px) {
    .info {
      flex-direction: row; align-items: center;
      gap: 16px;
    }
    .thumb {
      width: clamp(100px, 35vh, 160px);
      height: clamp(100px, 35vh, 160px);
      border-radius: clamp(8px, 2vh, 16px);
      box-shadow: 0 6px 24px rgba(0,0,0,0.65);
      flex-shrink: 0;
    }
    .info-text {
      flex: 1; overflow: hidden;
      display: flex; flex-direction: column; justify-content: center;
    }
    .title { font-size: clamp(14px, 2.6vh, 19px); }
    .artist { font-size: clamp(11px, 1.8vh, 14px); }
  }

  /* Hero layout — popup tall enough to show centered art */
  @media (min-height: 411px) {
    .info {
      flex: 1; min-height: 0;
      flex-direction: column; align-items: center; justify-content: center;
      gap: 16px; text-align: center;
    }
    .thumb {
      width: clamp(80px, 42vh, 340px);
      height: clamp(80px, 42vh, 340px);
      border-radius: clamp(8px, 2vh, 18px);
      box-shadow: 0 12px 40px rgba(0,0,0,0.7);
    }
    .info-text { text-align: center; width: 100%; }
    .scroll-clip { text-align: center; }
    .title { font-size: clamp(14px, 2.8vh, 24px); }
    .artist { font-size: clamp(11px, 1.8vh, 16px); }
  }
  /* Queue open: hide text, let thumb fill the info area in all 3 layout tiers */
  main.queue-open .info-text { display: none; }
  main.queue-open .info {
    flex-direction: column !important;
    justify-content: center;
    align-items: center;
  }
  main.queue-open .thumb {
    flex: 1 !important;
    min-height: 0;
    width: auto !important;
    height: auto !important;
    max-width: 100%;
    max-height: 100%;
    aspect-ratio: 1;
    border-radius: clamp(8px, 2vh, 18px) !important;
    box-shadow: 0 8px 28px rgba(0,0,0,0.7) !important;
  }

  .title.scrolling, .artist.scrolling {
    animation: marquee-scroll 3.5s ease-in-out infinite;
  }
  @keyframes marquee-scroll {
    0%,  12% { transform: translateX(0); }
    45%, 55% { transform: translateX(var(--marquee-d)); }
    88%, 100% { transform: translateX(0); }
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
  button {
    background: none; border: none; cursor: pointer;
    color: #f5f5f7; padding: 7px; border-radius: 8px;
    display: flex; align-items: center; justify-content: center;
    transition: background 0.15s;
  }
  button:hover  { background: rgba(255,255,255,0.09); }
  button:active { background: rgba(255,255,255,0.05); }

  /* Transport row: shuffle | prev | play | next | repeat */
  .controls {
    display: flex; align-items: center; justify-content: center; gap: 2px;
  }

  .play {
    background: var(--accent-dim);
    padding: 9px; margin: 0 6px; border-radius: 50%;
    transition: background 0.6s ease;
  }
  .play:hover { background: hsla(var(--h), var(--s), 45%, 0.38); }

  /* Action row: like | dislike — spacer — volume | queue */
  .actions { display: flex; align-items: center; gap: 2px; }
  .actions-spacer { flex: 1; }

  .icon-btn { color: rgba(255,255,255,0.45); padding: 6px; }
  .icon-btn:hover { color: rgba(255,255,255,0.75); background: rgba(255,255,255,0.07); }
  .icon-btn.active {
    color: #fff;
    background: rgba(255,255,255,0.13);
    border-radius: 7px;
  }

  .queue-toggle { display: flex; align-items: center; gap: 4px; }
  .queue-count  { font-size: 10px; font-weight: 600; }

  .repeat-btn { position: relative; }
  .repeat-one {
    position: absolute; inset: 0;
    display: flex; align-items: center; justify-content: center;
    font-size: 8px; font-weight: 800; line-height: 1;
    color: var(--accent);
    pointer-events: none;
  }

  /* Volume */
  .volume-section { display: flex; align-items: center; gap: 7px; padding: 0 1px; }
  .vol-mute {
    color: #636366; padding: 3px; flex-shrink: 0;
    background: none; border: none; cursor: pointer;
    display: flex; align-items: center; border-radius: 4px; transition: color 0.15s;
  }
  .vol-mute:hover { color: #f5f5f7; }
  .vol-pct { font-size: 10px; color: #636366; width: 28px; text-align: right; flex-shrink: 0; }

  /* Queue — frosted glass panel so cava/spectrum bars don't bleed into text */
  .queue {
    flex: none; max-height: 340px; overflow-y: auto; scrollbar-width: none;
    animation: panel-enter 80ms ease-out both;
    background: rgba(10, 10, 12, 0.78);
    backdrop-filter: blur(18px);
    -webkit-backdrop-filter: blur(18px);
    border-radius: 10px;
    padding: 8px 10px 6px;
    margin: 0 -3px -2px;
  }
  .queue::-webkit-scrollbar { display: none; }
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
  ul.with-thumbs li { grid-template-columns: 28px 1fr auto; padding-block: 3px; }
  li:hover          { background: rgba(255,255,255,0.06); }
  li.q-clickable    { cursor: pointer; }
  li.current        { background: var(--accent-dim); transition: background 0.6s ease; }

  .dot { font-size: 10px; color: #636366; text-align: center; line-height: 1; }
  li.current .dot { color: var(--accent); }
  .q-thumb {
    width: 28px; height: 28px; border-radius: 4px;
    object-fit: cover; flex-shrink: 0;
  }
  li.current .q-thumb { box-shadow: 0 0 0 1.5px var(--accent); }

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
    padding: 10px 12px 8px;
    flex: 1;
    animation: panel-enter 80ms ease-out both;
    background: rgba(10, 10, 12, 0.78);
    backdrop-filter: blur(18px);
    -webkit-backdrop-filter: blur(18px);
    border-radius: 10px;
    margin: 0 -3px -2px;
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

  @keyframes panel-enter {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

</style>
