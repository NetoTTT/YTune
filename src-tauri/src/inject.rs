pub const INJECT_JS: &str = r#"
(function() {
    // initialization_script runs in every frame — skip anything that isn't the main YTM page
    if (location.hostname !== 'music.youtube.com') return;
    if (window.__ytune_injected) return;
    window.__ytune_injected = true;

    console.log('[ytune] script active, __TAURI_INTERNALS__:', typeof window.__TAURI_INTERNALS__ !== 'undefined');

    window.__ytune__ = {
        playPause: () => document.querySelector('#play-pause-button button')?.click(),
        next:      () => document.querySelector('.next-button.ytmusic-player-bar button')?.click(),
        previous:  () => document.querySelector('.previous-button.ytmusic-player-bar button')?.click(),
        seek:      (t) => { const v = document.querySelector('video'); if (v) v.currentTime = t; },
        like: () => {
            const status = document.querySelector('#like-button-renderer')?.getAttribute('like-status');
            console.log('[ytune] like: current like-status=' + status);
            document.querySelector('#like-button-renderer .like button')?.click();
        },
        dislike: () => {
            const status = document.querySelector('#like-button-renderer')?.getAttribute('like-status');
            console.log('[ytune] dislike: current like-status=' + status);
            document.querySelector('#like-button-renderer .dislike button')?.click();
        },
        queueJump: (n) => {
            const items = document.querySelectorAll('ytmusic-player-queue-item');
            if (!items[n]) { console.warn('[ytune] queueJump: item not found n=' + n); return; }
            const el = items[n];

            // ytmusic-play-button-renderer has role="button" and display-style=PERSISTENT
            // (always in DOM, not just on hover). Clicking it starts playback of that item.
            const playBtn = el.querySelector('ytmusic-play-button-renderer');
            if (playBtn) {
                console.log('[ytune] queueJump: clicking play-button-renderer state=' + playBtn.getAttribute('state'));
                playBtn.click();
                return;
            }

            // Fallback: playerApi with delta-based index
            const api = document.querySelector('ytmusic-player')?.playerApi;
            if (api) {
                const currentDomIdx = Array.from(items).findIndex(i => i.hasAttribute('selected'));
                const pIdx = typeof api.getPlaylistIndex === 'function' ? api.getPlaylistIndex() : 0;
                const target = pIdx + (n - currentDomIdx);
                console.log('[ytune] queueJump fallback: playVideoAt(' + target + ')');
                api.playVideoAt(Math.max(0, target));
            }
        },
        setVolume: (v) => {
            // YTM stores its own volume state; setting video.volume directly gets overridden.
            // Use the player API so YTM updates its internal state too.
            const player = document.querySelector('ytmusic-player');
            if (player?.playerApi?.setVolume) { player.playerApi.setVolume(v); return; }
            const vid = getActiveVideo();
            if (vid) { vid.volume = Math.max(0, Math.min(1, v / 100)); vid.muted = (v === 0); }
        },
    };

    // ── Image processing (runs here in music.youtube.com context) ──────
    // fetch() from music.youtube.com works fine against Google CDN (same company,
    // CORS allowed). The popup at localhost:1420 cannot do this — CORS is blocked
    // and the failure poisons the browser cache, breaking even plain <img> display.
    // One fetch produces both: a data URI for display and the dominant palette color.
    let _palette   = { url: '', h: [280, 280, 280], s: [65, 65, 65] };
    let _thumbData = { url: '', data: '' };

    function ytimgFallbacks(src) {
        const m = src.match(/^(https:\/\/i\.ytimg\.com\/vi\/[^/]+\/)(maxresdefault|sddefault|hqdefault)(\.jpg)/);
        if (!m) return [];
        const order = ['maxresdefault', 'sddefault', 'hqdefault'];
        const idx = order.indexOf(m[2]);
        return order.slice(idx + 1).map(q => m[1] + q + m[3]);
    }

    function processImage(url) {
        if (!url || url === _palette.url) return;
        _palette.url = url;    // mark as in-progress to avoid duplicate calls
        _thumbData   = { url: '', data: '' }; // clear stale data from previous song

        function loadAndExtract(src) {
            fetch(src)
                .then(r => { if (!r.ok) throw new Error('HTTP ' + r.status); return r.blob(); })
                .then(blob => {
                    const blobUrl = URL.createObjectURL(blob);
                    const img = new Image();
                    img.onload = function() {
                        const SIZE = 60;
                        const c = document.createElement('canvas');
                        c.width = c.height = SIZE;
                        const ctx = c.getContext('2d');
                        const s = Math.min(img.naturalWidth, img.naturalHeight);
                        const sx = (img.naturalWidth - s) / 2;
                        const sy = (img.naturalHeight - s) / 2;
                        ctx.drawImage(img, sx, sy, s, s, 0, 0, SIZE, SIZE);
                        URL.revokeObjectURL(blobUrl);
                        _thumbData = { url: src, data: c.toDataURL('image/jpeg', 0.85) };
                        const { data } = ctx.getImageData(0, 0, SIZE, SIZE);
                        const bkts = Array.from({length:12}, () => ({n:0,h:0,s:0}));
                        for (let i = 0; i < data.length; i += 4) {
                            const r=data[i]/255, g=data[i+1]/255, b=data[i+2]/255;
                            const mx=Math.max(r,g,b), mn=Math.min(r,g,b);
                            const l=(mx+mn)/2;
                            if (mx===mn) continue;
                            const dv=mx-mn;
                            const s=l>0.5?dv/(2-mx-mn):dv/(mx+mn);
                            if (s<0.25||l<0.12||l>0.88) continue;
                            let h=0;
                            if(mx===r) h=((g-b)/dv+(g<b?6:0))/6;
                            else if(mx===g) h=((b-r)/dv+2)/6;
                            else h=((r-g)/dv+4)/6;
                            const bk=Math.min(Math.floor(h*12),11);
                            bkts[bk].n++;
                            if(s>bkts[bk].s){bkts[bk].h=h*360;bkts[bk].s=s;}
                        }
                        const dom = bkts.slice().sort((a,b)=>b.n-a.n);
                        _palette = {
                            url,
                            h: dom.slice(0,3).map(b => b.n>0 ? Math.round(b.h) : 280),
                            s: dom.slice(0,3).map(b => b.n>0 ? Math.round(b.s*100) : 65),
                        };
                        console.log(`[ytune] 🎨 palette from ${src}:`, JSON.stringify(_palette.h), JSON.stringify(_palette.s));
                    };
                    img.onerror = () => URL.revokeObjectURL(blobUrl);
                    img.src = blobUrl;
                })
                .catch(e => {
                    const fallbacks = ytimgFallbacks(src);
                    if (fallbacks.length > 0) {
                        console.warn('[ytune] processImage', e.message, src, '→ trying', fallbacks[0]);
                        loadAndExtract(fallbacks[0]);
                    } else {
                        console.warn('[ytune] processImage: all URLs failed for', url, e.message);
                    }
                });
        }

        loadAndExtract(url);
    }

    function getQueue() {
        const items = document.querySelectorAll('ytmusic-player-queue-item');
        if (items.length < 2) return [];
        const all = Array.from(items).map((el, idx) => ({
            title:    el.querySelector('.song-title')?.textContent?.trim() || '',
            artist:   el.querySelector('.byline')?.textContent?.trim() || '',
            current:  el.hasAttribute('selected'),
            domIndex: idx,
        }));
        const ci = all.findIndex(i => i.current);
        if (pollCount % 10 === 1) {
            console.log('[ytune] queue total=' + items.length + ' currentIdx=' + ci,
                all.slice(0,5).map(x => x.title + (x.current?' [CUR]':'')));
        }
        if (ci === -1) return [];
        return all.slice(Math.max(0, ci - 2), Math.min(all.length, ci + 4));
    }

    // Parse "m:ss" or "mm:ss" text to seconds
    function parseMmSs(text) {
        if (!text) return 0;
        const m = String(text).trim().match(/^(\d+):(\d{2})$/);
        return m ? +m[1] * 60 + +m[2] : 0;
    }

    // YTM uses a continuous DASH stream — video.currentTime and video.duration reflect
    // the position within the entire playlist stream, not the current song.
    // The player bar's time display always shows the correct per-song values.
    function getDisplayedTimes() {
        const el = document.querySelector('.time-info.ytmusic-player-bar');
        if (el) {
            const parts = el.textContent.split('/');
            if (parts.length === 2) {
                const cur = parseMmSs(parts[0]);
                const dur = parseMmSs(parts[1]);
                if (dur > 0) return { cur, dur };
            }
        }
        // Fallback: separate elements
        const curEl = document.querySelector('.current-time.ytmusic-player-bar');
        const durEl = document.querySelector('.duration.ytmusic-player-bar');
        const cur = parseMmSs(curEl?.textContent);
        const dur = parseMmSs(durEl?.textContent);
        return { cur, dur };
    }

    function getActiveVideo() {
        const all = Array.from(document.querySelectorAll('video'));
        // During song transitions YTM may have two video elements — old (paused at end)
        // and new (playing). Always prefer the one that is actually playing.
        return all.find(v => !v.paused && v.readyState >= 2 && v.duration > 0)
            || all.find(v => v.readyState >= 2 && v.duration > 0)
            || all[0]
            || null;
    }

    function getVideoId() {
        const el = document.querySelector('ytmusic-player');
        if (el) {
            const id = el.getAttribute('video-id') || el.getAttribute('videoId');
            if (id) return id;
        }
        const href = document.querySelector('ytmusic-player-queue-item[selected] a')?.href;
        if (href) {
            try { return new URL(href).searchParams.get('v') || ''; } catch(e) {}
        }
        try { const canon = document.querySelector('link[rel="canonical"]')?.href; if (canon) return new URL(canon).searchParams.get('v') || ''; } catch(e) {}
        try { return new URLSearchParams(window.location.search).get('v') || ''; } catch(e) { return ''; }
    }

    function cleanThumbUrl(url) {
        if (!url) return url;
        const qi = url.indexOf('?');
        return qi > 0 ? url.substring(0, qi) : url;
    }

    function upgradeThumbUrl(url) {
        if (!url) return url;
        const googleMatch = url.match(/^(.*=)w\d+-h\d+/);
        if (googleMatch) return googleMatch[1] + 'w640-h640';
        if (url.includes('i.ytimg.com')) {
            return url.replace(/\/hqdefault\.jpg/, '/maxresdefault.jpg')
                      .replace(/\/sddefault\.jpg/, '/maxresdefault.jpg')
                      .replace(/\/default\.jpg/, '/maxresdefault.jpg');
        }
        return url;
    }

    function getThumbUrl() {
        // Most reliable: Media Session API (set by YTM for all content types)
        try {
            const artwork = navigator.mediaSession.metadata?.artwork;
            if (artwork && artwork.length > 0) {
                const src = artwork[artwork.length - 1]?.src || artwork[0]?.src;
                if (src) return upgradeThumbUrl(cleanThumbUrl(src));
            }
        } catch(e) {}
        // Fallback: DOM selectors for album art
        const found = document.querySelector('ytmusic-player-bar ytmusic-thumbnail img')?.src
                   || document.querySelector('ytmusic-player-bar img[src*="googleusercontent"]')?.src;
        if (found) return upgradeThumbUrl(cleanThumbUrl(found));
        // Last resort: video ID from page elements
        const vid = getVideoId();
        if (vid) return 'https://i.ytimg.com/vi/' + vid + '/maxresdefault.jpg';
        return '';
    }

    function getState() {
        const video = getActiveVideo();
        const times = getDisplayedTimes();
        const thumb = getThumbUrl();
        // Fire-and-forget; updates _palette and _thumbData async for next poll cycle
        if (thumb && thumb !== _palette.url) processImage(thumb);
        return {
            title:       document.querySelector('.title.ytmusic-player-bar')?.title
                      || document.querySelector('.title.ytmusic-player-bar')?.textContent?.trim()
                      || '',
            artist:      document.querySelector('.subtitle.ytmusic-player-bar')?.textContent?.trim()
                      || '',
            thumbnail:   _thumbData.url || '', // empty until fetch confirms a working URL
            liked:       document.querySelector('#like-button-renderer')
                           ?.getAttribute('like-status') === 'LIKE',
            disliked:    document.querySelector('#like-button-renderer')
                           ?.getAttribute('like-status') === 'DISLIKE',
            playing:     video ? !video.paused : false,
            volume:      (function() {
                             const p = document.querySelector('ytmusic-player');
                             if (p?.playerApi?.getVolume) return p.playerApi.getVolume();
                             return video ? (video.muted ? 0 : Math.round(video.volume * 100)) : 100;
                         })(),
            currentTime: times.cur || 0,
            duration:    times.dur || 0,
            queue:       getQueue(),
            paletteH:      _palette.h,
            paletteS:      _palette.s,
            thumbnailData: _palette.url === thumb ? _thumbData.data : '',
        };
    }

    // ── Web Audio analyser ────────────────────────────────────────────
    // Runs here (music.youtube.com) because the <video> element lives in this frame.
    // Emits raw FFT bins to Rust at ~20fps; Rust relays as 'player-viz' to the popup.
    let _audioCtx   = null;
    let _analyser   = null;
    let _freqData   = null;
    let _vizRunning = false;
    let _audioTries = 0;

    function setupAudioAnalyser() {
        if (_audioCtx || _audioTries >= 8) return;
        _audioTries++;
        const video = getActiveVideo();
        if (!video || video.readyState < 2) return;
        try {
            const ctx = new (window.AudioContext || window.webkitAudioContext)();
            const src = ctx.createMediaElementSource(video);
            const an  = ctx.createAnalyser();
            an.fftSize = 64;               // 32 frequency bins
            an.smoothingTimeConstant = 0.55;
            src.connect(an);
            an.connect(ctx.destination);   // audio keeps playing normally
            ctx.resume().catch(() => {});
            _audioCtx  = ctx;
            _analyser  = an;
            _freqData  = new Uint8Array(an.frequencyBinCount);
            console.log('[ytune] Web Audio ready, bins:', an.frequencyBinCount);
        } catch(e) {
            console.warn('[ytune] Web Audio setup failed:', e.message);
        }
    }

    function startVizEmit() {
        if (_vizRunning) return;
        _vizRunning = true;
        function loop() {
            if (!_vizRunning) return;
            if (_analyser) {
                if (_audioCtx.state === 'suspended') _audioCtx.resume().catch(() => {});
                _analyser.getByteFrequencyData(_freqData);
                window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
                    event: 'ytune-viz',
                    payload: Array.from(_freqData),
                }).catch(() => {});
            }
            setTimeout(loop, 33); // ~30 fps
        }
        loop();
    }

    let pollCount = 0;
    function startPolling() {
        console.log('[ytune] polling started');
        setInterval(() => {
            const state = getState();
            pollCount++;
            // Lazy audio setup — requires a playing video and prior user gesture
            if (!_audioCtx) setupAudioAnalyser();
            if (_audioCtx && !_vizRunning) startVizEmit();
            if (pollCount <= 3) {
                const t = getDisplayedTimes();
                const tEl = document.querySelector('.time-info.ytmusic-player-bar');
                console.log('[ytune] probe: .time-info="' + tEl?.textContent?.trim() +
                    '" → cur=' + t.cur + ' dur=' + t.dur +
                    ' video.currentTime=' + getActiveVideo()?.currentTime?.toFixed(1));
            }
            if (pollCount % 10 === 1) {
                console.log('[ytune] poll #' + pollCount + ' title="' + state.title + '" playing=' + state.playing);
            }
            if (!state.title) return;
            if (pollCount <= 3) console.log('[ytune] getState thumb=' + JSON.stringify(state.thumbnail));
            window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
                event: 'ytune-state',
                payload: state,
            }).then(() => { if (pollCount <= 3) console.log('[ytune] emit OK', JSON.stringify({title: state.title, thumbnail: state.thumbnail, paletteH: state.paletteH, paletteS: state.paletteS})); })
              .catch(e => { if (pollCount % 10 === 1) console.error('[ytune] emit error:', e); });
        }, 1000);
    }

    function waitForPlayerBar() {
        if (document.querySelector('ytmusic-player-bar')) {
            console.log('[ytune] player-bar found immediately');
            startPolling();
            return;
        }
        console.log('[ytune] waiting for ytmusic-player-bar...');
        const obs = new MutationObserver(() => {
            if (document.querySelector('ytmusic-player-bar')) {
                console.log('[ytune] player-bar appeared');
                obs.disconnect();
                startPolling();
            }
        });
        obs.observe(document.documentElement, { childList: true, subtree: true });
        setTimeout(() => { obs.disconnect(); startPolling(); }, 10000);
    }

    // initialization_script runs before the DOM exists — wait for DOMContentLoaded
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', waitForPlayerBar);
    } else {
        waitForPlayerBar();
    }
})();
"#;
