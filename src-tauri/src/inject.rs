pub const INJECT_JS: &str = r##"
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
        seek: (t) => {
            // YTM uses a continuous DASH stream so video.currentTime is the absolute playlist
            // offset, not the song-relative time. Use playerApi.seekTo() which handles the
            // offset internally. Fall back to the progress-bar slider as a secondary option.
            const player = document.querySelector('ytmusic-player');
            if (player?.playerApi?.seekTo) {
                player.playerApi.seekTo(t, true);
                return;
            }
            // Fallback: drive YTM's own seek bar to the target percentage
            const dur = player?.playerApi?.getDuration?.() || 0;
            if (dur > 0) {
                const slider = document.querySelector('ytmusic-player-bar #progress-bar, ytmusic-player-bar tp-yt-paper-slider');
                if (slider) {
                    const pct = (t / dur) * (slider.max || 100);
                    slider.value = pct;
                    slider.dispatchEvent(new Event('change', { bubbles: true }));
                    return;
                }
            }
            // Last resort: direct video seek (may buffer on DASH streams)
            const v = document.querySelector('video');
            if (v) v.currentTime = t;
        },
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
                        if (window.__ytune__?.updateHeaderColors) window.__ytune__.updateHeaderColors(_palette.h[0], _palette.s[0]);
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
    // Emits raw FFT bins to Rust at ~30fps; Rust relays as 'player-viz' to the popup.
    let _audioCtx   = null;
    let _analyser   = null;
    let _freqData   = null;
    let _vizRunning = false;
    let _audioTries = 0;
    let _audioVideo = null; // video element the current analyser is connected to

    function teardownAudioAnalyser() {
        if (_audioCtx) { _audioCtx.close().catch(() => {}); }
        _audioCtx   = null;
        _analyser   = null;
        _freqData   = null;
        _audioVideo = null;
        _audioTries = 0;
    }

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
            _audioCtx   = ctx;
            _analyser   = an;
            _freqData   = new Uint8Array(an.frequencyBinCount);
            _audioVideo = video;
            _audioTries = 0; // reset on success so a future teardown can retry
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
            // Re-setup if the video element was swapped out or the context closed
            const activeVideo = getActiveVideo();
            if (_audioCtx && (activeVideo !== _audioVideo || _audioCtx.state === 'closed')) {
                console.log('[ytune] audio source changed, resetting analyser');
                teardownAudioAnalyser();
                _vizRunning = false;
            }
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
            window.__ytune__?.setVinylPlaying?.(state.playing);
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

    // ── Header buttons (notification bell + account circle) ──────────
    let _ytuneBtnStyle = null;

    window.__ytune__.updateHeaderColors = function(h, s) {
        if (!_ytuneBtnStyle) return;
        const h2 = (h + 35) % 360;
        _ytuneBtnStyle.textContent = `
            @keyframes ytune-disc-spin { to { transform: rotate(360deg); } }
            #ytune-player-btn .ytune-disc-g {
                transform-box: fill-box;
                transform-origin: center;
                animation: ytune-disc-spin 3s linear infinite;
                animation-play-state: paused;
                filter: sepia(1) hue-rotate(${(h-30+360)%360}deg) saturate(1.6) brightness(0.85);
            }
            #ytune-player-btn.ytune-playing .ytune-disc-g {
                animation-play-state: running;
            }
            #ytune-player-btn .ytune-arm-g {
                transform-box: view-box;
                transform-origin: 869px -25px;
                transform: rotate(-31deg);
                transition: transform 0.7s cubic-bezier(0.4,0,0.2,1);
                fill: hsl(${h2},${s}%,68%);
            }
            #ytune-player-btn.ytune-playing .ytune-arm-g {
                transform: rotate(-15deg);
            }
            #ytune-menu-btn {
                background: linear-gradient(145deg,hsl(${h},${s}%,18%),hsl(${h2},${s}%,14%)) !important;
                border: 1.5px solid hsla(${h},${s}%,55%,0.4) !important;
                color: hsl(${h},${s}%,82%) !important;
                box-shadow: 0 2px 8px hsla(${h},${s}%,30%,0.35) !important;
            }
            #ytune-menu-btn:hover {
                background: linear-gradient(145deg,hsl(${h},${s}%,26%),hsl(${h2},${s}%,20%)) !important;
                color: hsl(${h},${s}%,95%) !important;
            }
            #ytune-modal { border-color: hsla(${h},${s}%,55%,0.2) !important; }
            #ytune-modal-title {
                color: hsl(${h},${s}%,72%) !important;
                border-bottom-color: hsla(${h},${s}%,50%,0.15) !important;
            }
        `;
    };

    function makeSvg(pathD) {
        const NS = 'http://www.w3.org/2000/svg';
        const svg = document.createElementNS(NS, 'svg');
        svg.setAttribute('viewBox', '0 0 24 24');
        svg.setAttribute('width', '20');
        svg.setAttribute('height', '20');
        svg.setAttribute('fill', 'currentColor');
        const path = document.createElementNS(NS, 'path');
        path.setAttribute('d', pathD);
        svg.appendChild(path);
        return svg;
    }

    function makeHBtn(id, title) {
        const btn = document.createElement('button');
        btn.id = id;
        btn.title = title;
        btn.style.cssText = [
            'position:relative', 'display:flex', 'align-items:center',
            'justify-content:center', 'width:40px', 'height:40px',
            'border-radius:50%', 'background:transparent', 'border:none',
            'cursor:pointer', 'color:#aaa', 'transition:background 0.15s,color 0.15s',
            'flex-shrink:0',
        ].join(';');
        btn.addEventListener('mouseenter', () => {
            btn.style.background = 'rgba(255,255,255,0.1)';
            btn.style.color = '#fff';
        });
        btn.addEventListener('mouseleave', () => {
            btn.style.background = 'transparent';
            btn.style.color = '#aaa';
        });
        return btn;
    }

    function injectHeaderButtons() {
        const avatar = document.querySelector('#right-content ytmusic-settings-button');
        if (!avatar || document.getElementById('ytune-header-btns')) return;

        const wrap = document.createElement('div');
        wrap.id = 'ytune-header-btns';
        wrap.style.cssText = 'display:flex;align-items:center;gap:4px;margin-right:4px';

        // Notification bell
        const bellBtn = makeHBtn('ytune-notif-btn', 'ytune notifications');
        bellBtn.appendChild(makeSvg(
            'M12 22c1.1 0 2-.9 2-2h-4c0 1.1.9 2 2 2zm6-6v-5c0-3.07-1.63-5.64-4.5-6.32V4c0-.83-.67-1.5-1.5-1.5s-1.5.67-1.5 1.5v.68C7.64 5.36 6 7.92 6 11v5l-2 2v1h16v-1l-2-2z'
        ));
        const badge = document.createElement('span');
        badge.id = 'ytune-notif-badge';
        badge.style.cssText = [
            'position:absolute', 'top:6px', 'right:6px',
            'width:8px', 'height:8px', 'border-radius:50%',
            'background:#f44', 'border:1.5px solid #212121', 'display:none',
        ].join(';');
        bellBtn.appendChild(badge);
        bellBtn.addEventListener('click', () => console.log('[ytune] notification bell clicked'));

        // Account circle
        const accountBtn = makeHBtn('ytune-account-btn', 'ytune account');
        const _acctBtnSvg = makeSvg(
            'M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z'
        );
        const _acctBtnImg = document.createElement('div');
        _acctBtnImg.style.cssText = 'width:22px;height:22px;border-radius:50%;background-size:cover;background-position:center;display:none';
        accountBtn.appendChild(_acctBtnSvg);
        accountBtn.appendChild(_acctBtnImg);
        // Account header dropdown
        const _acctNS = 'http://www.w3.org/2000/svg';
        const accountDropdown = document.createElement('div');
        accountDropdown.id = 'ytune-account-dropdown';
        accountDropdown.style.cssText = [
            'position:fixed', 'z-index:10000',
            'background:#1e1e1e', 'border:1px solid rgba(255,255,255,0.1)',
            'border-radius:14px', 'padding:16px 20px',
            'box-shadow:0 8px 32px rgba(0,0,0,0.6)',
            'display:none', 'flex-direction:column', 'gap:10px',
            'font-family:Roboto,sans-serif', 'color:#fff',
            'min-width:180px', 'align-items:center',
        ].join(';');

        const _acctTitle = document.createElement('div');
        _acctTitle.style.cssText = 'font-size:11px;font-weight:600;color:rgba(255,255,255,0.4);text-transform:uppercase;letter-spacing:0.8px;align-self:flex-start';
        _acctTitle.textContent = 'ytune account';

        const _loginRow = document.createElement('div');
        _loginRow.id = 'ytune-login-row';
        _loginRow.style.cssText = 'display:flex;gap:14px;justify-content:center;padding:8px 0';

        function makeLoginCircle(bg, shadow, titleText, paths) {
            const btn = document.createElement('button');
            btn.title = titleText;
            const st = [
                'width:52px', 'height:52px', 'border-radius:50%',
                'background:' + bg, 'border:none', 'cursor:pointer',
                'display:flex', 'align-items:center', 'justify-content:center',
                'transition:transform 0.15s', 'flex-shrink:0',
            ];
            if (shadow) st.push('box-shadow:' + shadow);
            btn.style.cssText = st.join(';');
            btn.addEventListener('mouseenter', () => { btn.style.transform = 'scale(1.1)'; });
            btn.addEventListener('mouseleave', () => { btn.style.transform = 'scale(1)'; });
            const svg = document.createElementNS(_acctNS, 'svg');
            svg.setAttribute('viewBox', '0 0 24 24');
            svg.setAttribute('width', '26'); svg.setAttribute('height', '26');
            paths.forEach(function(item) {
                const p = document.createElementNS(_acctNS, 'path');
                p.setAttribute('d', item[0]); p.setAttribute('fill', item[1]);
                svg.appendChild(p);
            });
            btn.appendChild(svg);
            return btn;
        }

        const _discordCircle = makeLoginCircle('#5865F2', null, 'Login with Discord', [
            ['M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057c.002.022.015.04.034.052a19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03z', '#fff'],
        ]);
        _discordCircle.addEventListener('click', function() {
            window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
                event: 'ytune-open-url',
                payload: 'https://ytune.asktome.com.br/auth/discord',
            }).catch(function() {});
            accountDropdown.style.display = 'none';
        });

        const _googleCircle = makeLoginCircle('#fff', '0 2px 8px rgba(0,0,0,0.3)', 'Login with Google', [
            ['M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z', '#4285F4'],
            ['M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z', '#34A853'],
            ['M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l3.66-2.84z', '#FBBC05'],
            ['M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z', '#EA4335'],
        ]);
        _googleCircle.addEventListener('click', function() {
            window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
                event: 'ytune-open-url',
                payload: 'https://ytune.asktome.com.br/auth/google',
            }).catch(function() {});
            accountDropdown.style.display = 'none';
        });

        _loginRow.appendChild(_discordCircle);
        _loginRow.appendChild(_googleCircle);

        const _userInfoRow = document.createElement('div');
        _userInfoRow.id = 'ytune-user-info-drop';
        _userInfoRow.style.cssText = 'display:none;flex-direction:column;align-items:center;gap:6px;width:100%';

        const _userAvatar = document.createElement('div');
        _userAvatar.style.cssText = 'width:44px;height:44px;border-radius:50%;background:#5865F2;display:flex;align-items:center;justify-content:center;font-size:18px;font-weight:700;color:#fff';

        const _userNameEl = document.createElement('div');
        _userNameEl.style.cssText = 'font-size:14px;color:#fff;font-weight:500';

        const _logoutEl = document.createElement('div');
        _logoutEl.style.cssText = 'font-size:11px;color:rgba(255,255,255,0.35);cursor:pointer;padding:4px 12px;border-radius:20px;transition:background 0.15s';
        _logoutEl.textContent = 'Sign out';
        _logoutEl.addEventListener('mouseenter', function() { _logoutEl.style.background = 'rgba(255,255,255,0.08)'; });
        _logoutEl.addEventListener('mouseleave', function() { _logoutEl.style.background = 'transparent'; });
        _logoutEl.addEventListener('click', function() {
            localStorage.removeItem('ytune_token');
            _refreshAllAccountUI();
        });

        _userInfoRow.appendChild(_userAvatar);
        _userInfoRow.appendChild(_userNameEl);
        _userInfoRow.appendChild(_logoutEl);

        accountDropdown.appendChild(_acctTitle);
        accountDropdown.appendChild(_loginRow);
        accountDropdown.appendChild(_userInfoRow);

        function _refreshAllAccountUI() {
            const tok = localStorage.getItem('ytune_token');
            if (tok) {
                try {
                    const b64 = tok.split('.')[1].replace(/-/g, '+').replace(/_/g, '/');
                    const pay = JSON.parse(atob(b64));
                    const name = pay.username || pay.name || pay.sub || 'User';
                    if (pay.avatar) {
                        _userAvatar.textContent = '';
                        _userAvatar.style.backgroundImage = 'url(' + pay.avatar + ')';
                        _userAvatar.style.backgroundSize = 'cover';
                        _userAvatar.style.backgroundPosition = 'center';
                        _acctBtnImg.style.backgroundImage = 'url(' + pay.avatar + ')';
                        _acctBtnImg.style.display = 'block';
                        _acctBtnSvg.style.display = 'none';
                    } else {
                        _userAvatar.style.backgroundImage = '';
                        _userAvatar.textContent = name[0].toUpperCase();
                        _acctBtnImg.style.display = 'none';
                        _acctBtnSvg.style.display = '';
                    }
                    _userNameEl.textContent = name;
                    _userInfoRow.style.display = 'flex';
                    _loginRow.style.display = 'none';
                    if (pay.avatar) {
                        _mAcctAvatarEl.textContent = '';
                        _mAcctAvatarEl.style.backgroundImage = 'url(' + pay.avatar + ')';
                    } else {
                        _mAcctAvatarEl.style.backgroundImage = '';
                        _mAcctAvatarEl.textContent = name[0].toUpperCase();
                    }
                    _mAcctNameEl.textContent = name;
                    _mAcctSubEl.textContent = '@' + name;
                    _mAcctUserRow.style.display = 'flex';
                    _mAcctLoginRow.style.display = 'none';
                    return;
                } catch(e) {}
            }
            _userInfoRow.style.display = 'none';
            _loginRow.style.display = 'flex';
            _acctBtnImg.style.display = 'none';
            _acctBtnSvg.style.display = '';
            _mAcctSubEl.textContent = 'Login to scrobble';
            _mAcctUserRow.style.display = 'none';
            _mAcctLoginRow.style.display = 'flex';
        }
        window.__ytune__._refreshAllAccountUI = _refreshAllAccountUI;

        function _positionAccountDropdown() {
            const r = accountBtn.getBoundingClientRect();
            accountDropdown.style.top  = (r.bottom + 8) + 'px';
            accountDropdown.style.right = (window.innerWidth - r.right) + 'px';
        }

        accountBtn.addEventListener('click', function(e) {
            e.stopPropagation();
            const visible = accountDropdown.style.display === 'flex';
            accountDropdown.style.display = visible ? 'none' : 'flex';
            if (!visible) { _positionAccountDropdown(); _refreshAllAccountUI(); }
        });

        document.addEventListener('click', function(e) {
            if (!accountDropdown.contains(e.target) && e.target !== accountBtn)
                accountDropdown.style.display = 'none';
        });

// Open player button - vinyl turntable
        const playerBtn = document.createElement('button');
        playerBtn.id = 'ytune-player-btn';
        playerBtn.title = 'Open ytune player';
        playerBtn.style.cssText = 'position:relative;display:flex;align-items:center;justify-content:center;width:40px;height:40px;border-radius:50%;background:transparent;border:none;cursor:pointer;padding:0;flex-shrink:0;transition:transform 0.15s';
        playerBtn.addEventListener('mouseenter', () => { playerBtn.style.transform='scale(1.1)'; });
        playerBtn.addEventListener('mouseleave', () => { playerBtn.style.transform='scale(1)'; });
        playerBtn.addEventListener('click', () => {
            window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
                event: 'ytune-toggle-popup', payload: {},
            }).catch(() => {});
        });
        (function(){
        const NS="http://www.w3.org/2000/svg";
        const s=document.createElementNS(NS,"svg");
        s.setAttribute("viewBox","0 0 1024 1024");
        s.setAttribute("width","38");s.setAttribute("height","38");
        const dg=document.createElementNS(NS,"g");
        dg.setAttribute("class","ytune-disc-g");
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("fill","#555");
        p.setAttribute("d","M489.8 95.7c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5m40 0c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5m-52 1c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m-22 2c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m37 18c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5m36 0c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m11 51c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6M800.5 212c1 1.1 2 2 2.3 2s-.3-.9-1.3-2-2-2-2.3-2 .3.9 1.3 2m-277.7 10.7c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5m-315.9 7c-1.3 1.6-1.2 1.7.4.4.9-.7 1.7-1.5 1.7-1.7 0-.8-.8-.3-2.1 1.3m50.5 15.5-2.9 3.3 3.3-2.9c3-2.8 3.7-3.6 2.9-3.6-.2 0-1.6 1.5-3.3 3.2M746 261.5c1.9 1.9 3.6 3.5 3.9 3.5s-1-1.6-2.9-3.5-3.6-3.5-3.9-3.5 1 1.6 2.9 3.5m-472.6 4.7-1.9 2.3 2.3-1.9c2.1-1.8 2.7-2.6 1.9-2.6-.2 0-1.2 1-2.3 2.2M725 282.5c1.3 1.4 2.6 2.5 2.8 2.5.3 0-.5-1.1-1.8-2.5s-2.6-2.5-2.8-2.5c-.3 0 .5 1.1 1.8 2.5m-397.1 11.2c-1.3 1.6-1.2 1.7.4.4.9-.7 1.7-1.5 1.7-1.7 0-.8-.8-.3-2.1 1.3m31.5 52.5-1.9 2.3 2.3-1.9c2.1-1.8 2.7-2.6 1.9-2.6-.2 0-1.2 1-2.3 2.2M106.2 471c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m814.9.6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-227 1c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-572 1c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m7.2 2.4c0 2.5.2 3.5.4 2.2.2-1.2.2-3.2 0-4.5-.2-1.2-.4-.2-.4 2.3m85.9-1c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m-32.1.6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m30.1 1.4c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m60.9 1.6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m451 9c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m59 1c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3M244.2 493c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m29.9 2.6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-59 1c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m707 5c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m.1 5.4c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m-793.1.6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-22.9 3.4c0 1.9.2 2.7.5 1.7.2-.9.2-2.5 0-3.5-.3-.9-.5-.1-.5 1.8m737.9-.4c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3M654.2 514c0 1.9.2 2.7.5 1.7.2-.9.2-2.5 0-3.5-.3-.9-.5-.1-.5 1.8m-319.1-.4c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m263 5c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m.1 5.4c0 1.9.2 2.7.5 1.7.2-.9.2-2.5 0-3.5-.3-.9-.5-.1-.5 1.8m323 1c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m-345.9 7.5c0 2.7.2 3.8.4 2.2.2-1.5.2-3.7 0-5-.2-1.2-.4 0-.4 2.8m320.8 11.1c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m23 1c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3M349 662.4c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m-11 27c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m395.9 7.3-2.4 2.8 2.8-2.4c1.5-1.4 2.7-2.6 2.7-2.8 0-.8-.8-.1-3.1 2.4M304 699.4c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3M287.5 724c1 1.1 2 2 2.3 2s-.3-.9-1.3-2-2-2-2.3-2 .3.9 1.3 2m223.3 5.7c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m13 0c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5M231 744.4c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m300.8 14.3c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6M237 761.4c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m547.4 3.8-1.9 2.3 2.3-1.9c1.2-1.1 2.2-2.1 2.2-2.3 0-.8-.8-.2-2.6 1.9m-566.4.2c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m7.5 7.6c1 1.1 2 2 2.3 2s-.3-.9-1.3-2-2-2-2.3-2 .3.9 1.3 2m8 8c1 1.1 2 2 2.3 2s-.3-.9-1.3-2-2-2-2.3-2 .3.9 1.3 2m277.3 35.7c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5m19 0c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5m-9 2c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m-52 72c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m93 0c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m-82 1c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5m72 0c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m-57 1c1.2.2 3 .2 4 0 .9-.3-.1-.5-2.3-.4-2.2 0-3 .2-1.7.4m40.5 0c.9.2 2.3.2 3 0 .6-.3-.1-.5-1.8-.5-1.6 0-2.2.2-1.2.5");
        dg.appendChild(p);}
        const g1=document.createElementNS(NS,"g");
        g1.setAttribute("stroke-width","0");
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M489.5 95.6c-2.2.2-11.6 1.1-21 2-69.3 6.3-141.8 33.8-196.8 74.7-56.2 41.8-95.6 87.9-124.2 145.4-21.1 42.4-32.9 81.8-39.1 130.8-2.7 20.9-2.5 66.3.4 87.5 4.3 31.4 10.5 56.9 20.7 85.3 11.7 32.7 29.1 66.4 48.9 94.5 55.3 78.6 130.3 133.2 220.7 160.5 36.6 11.1 73.5 16.1 118.4 16.1 56.7 0 104.3-9.1 157.5-30.1 48-18.9 89.8-45.7 128.4-82.3 11.1-10.5 31.3-33.6 41.1-46.9 40.7-55.3 64.5-115.5 75.2-190.1 2.2-15.8 2.6-71 .5-86-5-36.1-10.3-60-19.3-86.5-18.6-54.4-46.3-101-85.5-143.6-43-46.7-108-89.1-167.4-109.2-32.4-11-64.6-18-94.5-20.7-12.8-1.1-57.3-2.1-64-1.4m51 14.4c57.3 4.2 117.5 22.3 165.1 49.5C783.2 204 841.4 268.2 875.2 347c16.2 37.6 26.5 79 29.9 119.5 1.7 20.2.6 60.9-2.1 80.4-10 73.3-39.5 139.3-87.6 196.6-32.1 38.2-79.8 74.1-128.5 96.6-64.5 29.8-137.8 42.4-208.4 35.8-88.4-8.3-172.1-47.1-235.4-109.2-24.7-24.3-40.3-43.1-57.4-69.4-32.5-49.9-53.3-104.8-60.9-160.8-3.2-23.4-3.2-68.7.1-92.5 11.6-85.4 50.3-162.1 112.2-222.6 45.4-44.4 96.9-75.1 157.2-93.8 49.6-15.4 97.3-21.1 146.2-17.6");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M491.5 116.7c-74.1 4.3-149.1 30.8-205.5 72.7-52.9 39.3-93.8 87.8-120 142.1-21.9 45.3-33.3 89.4-36.1 139.5-4.7 83.8 20.2 165.3 72.6 238 22.9 31.8 57.4 66.3 88.5 88.7 52.3 37.7 111.2 61.2 173.5 69.3 27.4 3.5 63.8 4.2 89.5 1.5 124.3-12.6 228.5-76.4 290.7-177.9 34.8-56.8 52.3-116.1 54-183.1 1.8-72.4-16.2-142.1-52.5-202.5-36.5-60.7-95.7-115.2-160.3-147.6-24.4-12.2-42.5-19.1-71.4-27.4-37.5-10.8-82.8-15.7-123-13.3M537 138c20.1 1.4 34.8 3.5 53.5 7.7 95.2 20.9 178.1 78.9 229.8 160.7 26.2 41.5 45 92.5 53.3 145.1 2.5 15.5 2.7 19.5 2.8 44.5 0 28.7-1 40-5.5 63-19.4 99.3-77.7 182.7-164.6 235.3-38.9 23.6-81.6 39.5-126.3 47.1-26.6 4.5-34.2 5.1-65.5 5.1-25.7-.1-32.2-.4-45.4-2.3-70.4-10.4-123.7-33.2-177.2-76-54.9-44-98.5-107.1-121.4-175.7-13.6-40.7-18.2-70.3-17.2-111 1.2-51.5 13.2-98.5 37-146.2 18-35.7 36.7-61.5 66.1-90.8 35.9-35.9 67.5-57.6 113-77.7C419 144.9 481.5 134.1 537 138");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M492 139.6c-32.2 3-51 6.1-74.9 12.5-41.1 11-85.3 32.5-117.9 57.3-20.2 15.4-45.2 38.7-60.2 56.1-43.7 50.8-72.9 116.4-81.6 183-2.2 16.9-2.9 50.1-1.5 66.2 3.7 41.8 15.5 82.7 35.8 124 35.3 72.1 92.2 131.3 160 166.5 38.1 19.9 75.3 31.4 121.3 37.5 16.8 2.2 65.6 2.5 82 .5 47.9-5.9 85.8-17 124.4-36.4 36.7-18.4 64.2-38.2 93.7-67.7 35.1-35.1 58.4-70.5 77.4-117.7 17-41.9 25.8-91.3 24.1-134.9-1.6-42.7-12-86.9-30.6-130.5-30-70.2-83.5-129.9-151-168.7-43.4-25-89.6-40.2-139.1-45.8-13.1-1.5-52.9-2.7-61.9-1.9m54.8 26.5c43.9 4.7 77 14.4 116.7 33.9 33.8 16.7 56.3 32.8 83.1 59.5 32.1 32 55.7 66.9 73.2 108 34.4 80.8 36 168.8 4.5 246.5-21.1 52.3-54.4 96.9-98.1 131.8-27.1 21.6-54.9 37.6-87.7 50.5-41.3 16.3-89.3 24-134.5 21.6-61.8-3.4-118.5-22.1-169.4-56.1-45.7-30.5-88.6-78.3-113.9-126.9-19.6-37.8-30.7-74.4-35.8-117.9-1.8-15.5-1.6-51.8.5-67.5 7.7-58.8 31.7-115 68.6-160.9 38.2-47.5 89.8-84 147-104.1 29.6-10.3 47.1-14.2 85.5-18.8 7.1-.9 51.4-.6 60.3.4");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M478 168.5c-57.7 7.1-111.3 27.3-156 58.8-67.8 47.7-114.5 119.1-130.9 200.3-4.2 21.2-5.5 34.9-5.4 58.4.2 42.7 7.8 81.1 23.6 119.7 26.4 64.4 75.4 122.5 135.2 160.2 66 41.6 150.8 59.2 225.2 46.6 56.2-9.5 97.2-26.3 142.2-58.3 64-45.4 109.3-114.2 126-191.5 5.7-26.1 8-56.9 6.2-82.5-5.1-73.1-28.9-134.8-74.8-193.1-11.4-14.5-40.6-43-55.5-54.1-39.6-29.7-89.6-51.9-135.8-60.4-21.1-3.9-30.9-4.8-59.5-5.1-20.9-.3-32.2 0-40.5 1m76 28c32.8 4.4 56 10.8 83 23 33.8 15.2 63.1 35.4 89.6 61.9 24 24 39.6 45.9 55.7 78.1 30.3 60.6 40.3 119.6 31.1 183.5-4 28-17.3 66.2-32.5 93.5-13.8 24.9-28.6 44.4-49.3 65.1-15.7 15.7-25.8 24-42.1 34.9-36.6 24.4-69 37.8-112 46.4-37.5 7.5-74.4 7.8-112.1 1C385.1 769.5 320.2 727.7 271 659c-28.4-39.7-46.2-81.1-53.5-124.2-5-30.2-5.5-55.9-1.5-86.8 10-77.5 49.9-145 114.6-193.7 22.7-17.2 53.5-33.4 83.7-43.9 18.1-6.4 43.6-12 65.2-14.3 5.5-.6 11.6-1.3 13.5-1.5 7.7-.9 50.1.4 61 1.9");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M491.5 196.6c-47.4 4.6-84.4 15.4-122.5 35.7-53.6 28.6-98.2 74.5-124.6 128.1-36.5 74.3-38.6 163.4-5.6 238.2 26.2 59.3 69.7 110.7 121.3 143.2 43.1 27.1 95.1 43 147.4 44.8 87.4 3.2 172.3-31.4 230.4-94 29.7-32 53.2-73.4 65.5-115.1 19.5-66.6 14-135.3-16.3-203-24.8-55.6-59.5-97.3-107.7-129.3-39-26-76.9-40.2-124.4-46.8-10.9-1.5-54-2.7-63.5-1.8m57.5 27.8c40.7 5.1 79.1 19.1 112 40.7 37 24.3 65 53.7 85.9 90.2 20 34.7 32.4 71.1 37.8 110.7 2.5 18.8 2.3 52.4-.5 69.7-10.7 66-43.8 124.6-92.3 163.3-58.4 46.7-138.2 68-213.4 57-30.1-4.4-55.8-12.3-81.4-25.1-32.1-15.9-58.3-35.8-81.3-61.6-57.2-64.1-83-146.1-70.2-223.8 9.1-55.6 32.6-101.1 73.7-142.9 36.6-37.1 85-63.5 136.7-74.5 9.8-2 29.3-4.8 39-5.5 10.2-.7 42.4.4 54 1.8");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M491.5 224.9c-22.5 1.8-52.2 8.2-72.1 15.6-14.9 5.6-42.5 19.4-54.9 27.5-28.8 18.9-58.5 48.2-77.3 76.3-46.3 69.2-55.4 158.2-24.2 236.3 15.2 38 35.8 69.4 64.4 98 15.6 15.5 24.6 22.9 41.5 34.1 42.2 28 88.6 42.3 142.6 43.9 77.4 2.4 156.7-29.9 205.3-83.6 17.9-19.8 29.1-36.4 40.8-60.4 20.8-42.7 29.5-84.7 26.5-127.9-2.9-42.4-12.4-77-31.8-114.9-13.4-26.4-26.4-44.7-46.2-65-43.1-44.5-97.3-71.3-159.6-78.8-12.2-1.4-42.7-2-55-1.1M555 254c48.5 8.6 94.3 32.4 128.7 66.7 36.4 36.4 61.4 86.9 69.8 141.3 2.3 14.6 3.1 46.6 1.6 61.3-6.9 67-44.9 129.8-101 167-28.7 18.9-65.6 32.4-102.6 37.3-18.8 2.5-50.5 2.3-69.3-.5-63.4-9.2-113.7-38.3-155.7-90.1-10.5-13-19.8-27.8-29-46.5-12.3-25.1-19-46.1-23.1-73-2.3-14.9-2.3-45.9 0-61.6 6-42.1 23.2-82.1 49.1-114.4 7.1-8.9 23.7-25.7 34-34.4 33.9-28.7 78.5-48 126.5-54.6 16.3-2.2 54.8-1.4 71 1.5");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M486 254.5c-42.3 5.1-80.6 19.8-113.4 43.3-25.4 18.3-49.6 44.1-64.6 69.2-33.4 55.7-42.2 119.5-24.8 181.5 12.6 45.4 40.9 89.9 76.7 120.9 24.3 20.9 45.9 33.7 74.1 43.9 68.4 24.6 149.7 17.2 210.9-19.4 29.3-17.6 60.8-49.4 77.4-78.4 23.1-40.2 32.7-77.8 31.4-122.3-.6-22.4-2.5-36.4-7.6-56.6-16.6-65.7-56.3-119.2-112.6-151.9-28.1-16.2-61.3-27.2-92.7-30.6-12.8-1.4-41.7-1.2-54.8.4m45.5 26.6c40.7 3.5 71.6 14.9 105 38.5 12.1 8.5 30.3 25.7 39.8 37.4 24.5 30.3 42.2 71.1 47.2 108.8 8.2 61.3-5.7 115-41.3 160-36.6 46.3-101.1 76.3-163.9 76.2-57.4-.2-110.3-21.9-150.3-62-30.9-30.9-53.8-72.5-62.6-113.8-3.5-16.2-4.4-45.3-2-62.7 5.4-38.9 22.9-77.4 50.4-110.5 12.7-15.4 31.8-31.6 50.7-43 36.9-22.3 82.9-32.8 127-28.9");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M500 282.6c-1.9.2-7.8.9-13 1.4-77.1 8.4-143.1 61.5-171 137.8-9.4 25.5-12.5 45.3-11.7 73.2 1.2 38 12 71.2 34.4 105.4 18.4 28.2 36.9 47.1 63.3 64.9 34.2 23 74 34.7 117.8 34.7 46.5 0 96-17.5 132.2-46.7 23.9-19.3 43.5-45.4 55.9-74.5 14.6-34.1 19.3-73.2 13.6-112.7-11.5-79.4-63.5-145-136.3-171.7-11.5-4.2-26.4-8-39-10-9.6-1.4-39.4-2.6-46.2-1.8m31 26.9c20.4 1.4 44.7 8.3 64.2 18.1 34.4 17.3 61.6 44.5 79.7 79.5 27.1 52.7 27.5 112.1 1.1 164.5-21.4 42.4-58 74.3-103.5 90-18.9 6.6-34.1 8.8-59 8.9-32.6 0-51.2-4.2-77.6-17.5-45.3-22.8-82.1-67.2-95.4-115-5.1-18.3-5.9-25.1-5.9-49.5-.1-20.2.2-24.3 2.2-33.8 7.9-37.6 26.2-71.1 52.8-96.7 15-14.4 26.3-22.2 45.4-31.4 29.9-14.4 60-19.7 96-17.1");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M505.2 331.2c-8.3 4-18 19.9-22.3 36.3-4.2 16.2-3.7 40.2 1.2 60.3 1.7 6.7 1.8 8.6.8 9.8-.7.8-5.5 5-10.8 9.2-19.2 15.4-32.5 31-39.6 46.5-5.3 11.7-7 19.6-6.9 33.2 0 11.1.3 12.8 3.5 22 10 29 31.4 44.8 65.6 48.6 10.4 1.1 13.3.4 13.3-3.6 0-2.8-2.3-5.3-5.3-5.9-14.8-2.8-20.2-5-28.7-11.3-15.3-11.4-24.2-32.7-21-50.3 2.4-13 8.6-24.4 20.1-36.4 6.5-6.9 23-21.6 24.1-21.6.6 0 3.1 11.1 2.6 11.4-.1.1-3.8 2.2-8.2 4.7-15.7 8.8-26.2 24.9-26.7 40.9-.2 5 .1 6.6 1.6 7.6 3.2 2.4 5.1.2 5.9-6.8 1.5-14.5 11.6-28.3 26-35.4 7.9-3.9 8.1-3.9 19.5-3.9 9.7 0 12.5.4 17.3 2.3 13 5.2 23.7 16.7 27.5 29.2 2.3 7.6 2.2 21-.1 28-5.4 16.3-16.5 29.4-30.5 36.1-7.1 3.4-8.7 5.2-7.5 8.4.6 1.3 1.5 1.6 4.4 1.2 12.2-1.6 30.6-16.7 37.3-30.7 5-10.2 6.7-17.7 6.7-29 0-16.8-4.3-27.4-15.6-38.8-11.5-11.6-26.1-17.1-41.8-15.9l-7.4.5-1.2-4.6c-.7-2.6-1.9-5.7-2.7-7-1.4-2.2-1.1-2.7 6.4-10.5 14.3-14.9 22.1-27.5 26.9-43.2 2.6-8.3 2.8-10.3 2.8-26 0-15.6-.3-17.7-2.7-25.7-3-9.9-7.4-19.4-11.3-24.5-5.3-6.9-15.2-9.1-23.2-5.1m14.6 8.3c4 3.4 9.6 14.4 12.1 24 2.9 11 3.7 27.9 1.8 37.6-2.2 11.1-7 22.4-13.6 32.5-6.4 9.7-14.1 17.5-32.1 32.5-24.7 20.6-36.3 36.6-40.6 55.7-2.1 9.2-1.5 20.9 1.5 30.3 2.7 8.4 10.2 20 17 26.1 6 5.4 5.6 6.2-1 2-6.8-4.4-11.4-8.7-16.2-15.5C439.5 551.9 436 541 436 526c0-12.1 1.7-18.9 8-31.5 6.1-12.4 14.6-22.5 31.2-37.2 29-25.7 37.4-34.3 44.4-45.5 10.8-17.6 12.5-41.1 3.6-50.9-2.9-3.4-4-3.9-7.7-3.9-8.1 0-15.1 7.9-19.6 22-2.8 8.5-3.5 28-1.6 40.3 1 7.1 1.1 9.2.1 10.5-1.1 1.5-1.3 1.5-1.8.2-3.6-9.4-5.7-37.9-3.7-50.7 2.6-16.9 10-32.8 18-39.1 5.1-3.9 8.9-4.1 12.9-.7m-.6 28.5c2.2 4.4 2.4 15.4.3 23.3-1.9 7.3-7.6 18.7-12.8 25.7l-4.2 5.5-.7-4.5c-.4-2.5-.8-9.5-.8-15.5-.1-15.5 3.2-27.1 10-34.8 3.9-4.5 5.9-4.4 8.2.3m87 53.2c-2.3 2.3-1.1 4.4 6.8 12.3 4.4 4.4 10.1 11 12.5 14.6 6.8 10.2 14.1 26.3 17.2 37.9 2.6 9.7 2.8 11.9 2.7 28.5 0 16.3-.3 18.9-2.8 28-3.3 12.2-11.7 29.6-18.8 38.9-2.9 3.8-5.7 7.5-6.3 8.2-1.3 1.8.5 4.8 3.2 5.2 3.8.6 16.6-17.4 24-33.7 7.1-15.8 8.8-25.3 8.7-47.6 0-17.2-.3-19.9-2.7-28.7-5.8-21.7-17.3-42.3-31.8-57.1-7.5-7.6-10.2-9-12.7-6.5");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M595.2 437.2c-2.2 2.2-1.2 4.3 4.9 10.3 33.3 32.6 37.2 88 8.7 124.6-3.3 4.3-3.5 6.4-.9 7.9 2.7 1.4 4.1.6 7.8-4.3 16.1-21.4 23.4-50.5 19.2-76.7-1.3-8.5-7.1-25.8-11-32.9-5.2-9.5-14.1-21.2-19.7-25.7-5.7-4.6-7.1-5.1-9-3.2");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M582.2 452.2c-2.2 2.2-1.2 4.1 5.6 11.2 24.9 25.8 29.1 66.1 10 95.4-4.4 6.8-4.4 9.4 0 10 2.6.4 6.9-5.2 11.7-15.3 12.9-27 9.8-61.1-7.9-85.8-4.9-7-14.8-16.7-16.8-16.7-.8 0-1.9.5-2.6 1.2");
        g1.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M566.9 470.1c-1 2-.5 2.8 4.6 7.8 7.8 7.5 13 15.8 16 25.4 4.8 15.1 4.2 28.2-2 42.9-4.1 9.7-4.3 11.6-1 12.4 3.2.8 4.5-.5 7.7-7.6 3.9-8.7 5.8-18 5.9-29.1 0-12.5-1.7-20.2-6.7-30.3-4.1-8-5.5-10.1-13.1-17.9-6.1-6.2-9.4-7.3-11.4-3.6M501 500.5c-1.9 2.3-1.4 54.2.6 57.2.8 1.3 1.9 2.3 2.5 2.3 1.5 0 38.4-19.3 44.5-23.3 3.7-2.4 5-3.9 5.2-6 .6-4.7.6-4.7-30-22.1-17.2-9.8-20.4-10.9-22.8-8.1m13 64c-1.2 1.5-.8 7.8 2.5 39 .8 7.7 1.5 16.8 1.5 20.2 0 11.5-6.2 22.8-15.1 27.4-4.2 2.1-15.6 4-18.9 3.2-1.7-.5-1.5-.8 1.9-2.5 4.7-2.3 10.7-8.7 12.7-13.4 1.8-4.3 1.8-11.5 0-15.8-2.2-5.2-7.3-10.3-12.4-12.3-16.9-6.4-33.2 5.3-33.2 23.9 0 12.2 10 22.9 24.7 26.4 19.3 4.5 37.8-3.2 44.7-18.6 3.7-8.1 4.2-17.7 2.2-35.5-.9-7.7-2.1-20-2.6-27.4-.6-7.4-1.5-14.1-2.1-14.8-1.5-1.7-4.4-1.7-5.9.2m-30 53.3c5.5 2.7 8.3 7.7 7.8 13.9-.6 7.4-4.2 11.8-11.4 13.9-5 1.4-5.6 1.8-5.1 4 .6 3.5-1.3 3-6.8-1.8-5.5-4.9-7.5-8.9-7.5-15.3 0-12.5 12.1-20.2 23-14.7");
        g1.appendChild(p);}
        dg.appendChild(g1);
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("fill","gray");
        p.setAttribute("d","M511.3 166.7c.9.2 2.3.2 3 0 .6-.3-.1-.5-1.8-.5-1.6 0-2.2.2-1.2.5m2 86c.9.2 2.3.2 3 0 .6-.3-.1-.5-1.8-.5-1.6 0-2.2.2-1.2.5M183.2 479c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m120.1 9c0 2.5.2 3.5.4 2.2.2-1.2.2-3.2 0-4.5-.2-1.2-.4-.2-.4 2.3M342 655.4c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m342.4 7.8-1.9 2.3 2.3-1.9c1.2-1.1 2.2-2.1 2.2-2.3 0-.8-.8-.2-2.6 1.9m-170.6 66.5c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5m261.1 8c-1.3 1.6-1.2 1.7.4.4s2.1-2.1 1.3-2.1c-.2 0-1 .8-1.7 1.7m-267.1 79c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m28 30c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6");
        dg.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("fill","#aaa");
        p.setAttribute("d","M500.8 137.7c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5m7 29c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m9 0c1.2.2 3 .2 4 0 .9-.3-.1-.5-2.3-.4-2.2 0-3 .2-1.7.4m-11.5 56c.9.2 2.5.2 3.5 0 .9-.3.1-.5-1.8-.5s-2.7.2-1.7.5m-240.9 15.5-1.9 2.3 2.3-1.9c2.1-1.8 2.7-2.6 1.9-2.6-.2 0-1.2 1-2.3 2.2m245.4 14.5c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6M715 270.4c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m4 6c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m-420.6 7.8-1.9 2.3 2.3-1.9c2.1-1.8 2.7-2.6 1.9-2.6-.2 0-1.2 1-2.3 2.2M215.2 470c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m28.9 3.6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m30 1c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-32 4c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-58.8 5.9c0 2.2.2 3 .4 1.7.2-1.2.2-3 0-4-.3-.9-.5.1-.4 2.3m29.9-1.5c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m59.1 3.5c0 3.3.2 4.5.4 2.7s.2-4.5 0-6-.4 0-.4 3.3m-30 1c0 2.2.2 3 .4 1.7.2-1.2.2-3 0-4-.3-.9-.5.1-.4 2.3m603.9-.5c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m-121.1 4.6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-512 2c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m90 2c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m543 14c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3M367 639.4c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m313.9 27.3c-1.3 1.6-1.2 1.7.4.4s2.1-2.1 1.3-2.1c-.2 0-1 .8-1.7 1.7M323 677.5c1.3 1.4 2.6 2.5 2.8 2.5.3 0-.5-1.1-1.8-2.5s-2.6-2.5-2.8-2.5c-.3 0 .5 1.1 1.8 2.5m344.3 2c0 .8.4 1.2.9.9s.6-1 .3-1.5c-.9-1.3-1.2-1.1-1.2.6M296 688.4c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m468.6 18.4c-.3.5.1.9.9.9s1.2-.4.9-.9c-.3-.4-.7-.8-.9-.8s-.6.4-.9.8m-246.3 22.9c.9.2 2.3.2 3 0 .6-.3-.1-.5-1.8-.5-1.6 0-2.2.2-1.2.5M261 738.4c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m254.8 107.3c.6.2 1.8.2 2.5 0 .6-.3.1-.5-1.3-.5s-1.9.2-1.2.5");
        dg.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("fill","#d4d4d4");
        p.setAttribute("d","M508.8 137.7c2.8.2 7.6.2 10.5 0 2.8-.2.5-.3-5.3-.3s-8.1.1-5.2.3m-15 28c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m43 0c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m-34.5 1c.9.2 2.5.2 3.5 0 .9-.3.1-.5-1.8-.5s-2.7.2-1.7.5m21 0c.9.2 2.5.2 3.5 0 .9-.3.1-.5-1.8-.5s-2.7.2-1.7.5m1.5 28c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m-12.5 28c1.5.2 3.7.2 5 0 1.2-.2 0-.4-2.8-.4-2.7 0-3.8.2-2.2.4m-236 3.8c0 .8.4 1.2.9.9s.6-1 .3-1.5c-.9-1.3-1.2-1.1-1.2.6m475.3 2.7c.3.5 1 .6 1.5.3 1.3-.9 1.1-1.2-.6-1.2-.8 0-1.2.4-.9.9m-485 4.7c-.4.5-.2 1.2.3 1.5.5.4 1.2.2 1.5-.3.4-.5.2-1.2-.3-1.5-.5-.4-1.2-.2-1.5.3m496.7 4.6c0 .8.8 2.2 1.7 3 1.4 1.3 1.7 1.3 1.7.1 0-.8-.5-1.4-1.1-1.3-.5.1-1-.3-1.1-1 0-1.7-1.2-2.5-1.2-.8m7 6.1c.9 1.1 1.5 2.3 1.2 2.6-.7.7 2.3 3.2 4.2 3.4 1.1.2 1.2-.1.3-1.1-.7-.8-1.7-1.3-2.3-1.2-.7.1-1.1-.3-1-.9.1-.7-.8-2-2-3-2.2-1.9-2.2-1.9-.4.2m-34.8 6.5c1 1 2.2 1.6 2.7 1.3 1.2-.8-.6-2.7-2.8-3-1.4-.2-1.4 0 .1 1.7m-229.7 1.6c.7.3 1.6.2 1.9-.1.4-.3-.2-.6-1.3-.5-1.1 0-1.4.3-.6.6m-259.5 3c0 .9.4 1 1.4.2.7-.6 1.3-1.2 1.3-1.4 0-.1-.6-.2-1.3-.2-.8 0-1.4.6-1.4 1.4m32.9 4.7c-1.7 2.7-.9 3.6 1.4 1.5 1.3-1.2 2.4-2.3 2.4-2.4 0-.7-3.2.1-3.8.9m504.4.8c.3.5 1 .6 1.5.3 1.3-.9 1.1-1.2-.6-1.2-.8 0-1.2.4-.9.9m-546 4c.3.5 1 .6 1.5.3 1.3-.9 1.1-1.2-.6-1.2-.8 0-1.2.4-.9.9m516.8 4.6c1.8 2.1 2.9 2.3 2.7.5-.1-.4-.4-.8-.8-.8-.5 0-1.5-.4-2.3-.9-.9-.6-.7-.1.4 1.2m-522.8 1c-.3.5.1.9.9.9s1.2-.4.9-.9c-.3-.4-.7-.8-.9-.8s-.6.4-.9.8m486.7 3.7c0 .8.4 1.2.9.9s.6-1 .3-1.5c-.9-1.3-1.2-1.1-1.2.6m-454.2 1.8c-.8 1-.7 1.4.2 1.4.8 0 1.4-.6 1.4-1.4 0-.7-.1-1.3-.2-1.3-.2 0-.8.6-1.4 1.3m36.4 3.7c-1.3 1.4-1.3 1.7-.1 1.7.8 0 1.4-.5 1.3-1.1-.1-.5.3-1 1-1.1 1.7 0 2.5-1.2.8-1.2-.8 0-2.2.8-3 1.7m467 3.9c-.3.5-.2 1.2.3 1.5s.9-.1.9-.9c0-1.7-.3-1.9-1.2-.6m-473 2.9c.3.3-.3 1.5-1.2 2.6-1.3 1.5-1 1.4 1-.3 3.2-2.9 3.1-2.8 1.2-2.8-.8 0-1.3.3-1 .5m437.9 3c.4.3 1.4.7 2.2.8.9.1 1.3 0 1-.4-.4-.3-1.4-.7-2.2-.8-.9-.1-1.3 0-1 .4m-32.4 8.7c1.3 1.4 2.6 2.5 2.8 2.5.3 0-.5-1.1-1.8-2.5s-2.6-2.5-2.8-2.5c-.3 0 .5 1.1 1.8 2.5m-380.9 3.9c-.9 1.1-.9 1.6.1 2.2.7.4 1 .4.6-.1-.4-.4-.3-1.4.3-2.2.6-.7.9-1.3.6-1.3-.2 0-1 .6-1.6 1.4m385.8.9c.8.9 1.6 1.5 1.9 1.2s1.5.3 2.6 1.2c2.1 1.8 2.1 1.8.2-.5-1.1-1.2-2.1-2-2.4-1.7s-1.2-.1-2.1-.8c-1.4-1.1-1.4-1-.2.6m-40.3 2.9c.3.5 1 .6 1.5.3 1.3-.9 1.1-1.2-.6-1.2-.8 0-1.2.4-.9.9m27.7 25.3c0 .8.4 1.2.9.9s.6-1 .3-1.5c-.9-1.3-1.2-1.1-1.2.6m-366.8 7.4c-.3.5-.2 1.2.3 1.5s.9-.1.9-.9c0-1.7-.3-1.9-1.2-.6m38 3c-.3.5-.2 1.2.3 1.5s.9-.1.9-.9c0-1.7-.3-1.9-1.2-.6M215.1 466.6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-30 4c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m118 10c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-60.9 1.4c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m-59 8c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m-29 1.5c0 1.6.2 2.2.5 1.2.2-.9.2-2.3 0-3-.3-.6-.5.1-.5 1.8m58.9-.9c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m633 0c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-60 5c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-483 3c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-88 4c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m661.1 3.4c0 1.4.2 1.9.5 1.2.2-.6.2-1.8 0-2.5-.3-.6-.5-.1-.5 1.3m-30.1.6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-60 1c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m30 3c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-32 6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m60 0c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-91 6c0 1.1.3 1.4.6.6.3-.7.2-1.6-.1-1.9-.3-.4-.6.2-.5 1.3m-82.4 113.3c-1 1.4-1 1.4.4.4.9-.7 1.8-1 2.1-.7s.8.1 1.2-.4c.9-1.6-2.4-.9-3.7.7m-305.7-.5c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m299.1 7c-.8 1-.9 1.6-.1 2.1.5.3 1 .5 1.1.3.4-1.8.7-3.8.4-3.8-.1 0-.8.6-1.4 1.4m-328.8 4.1c0 .8.4 1.2.9.9s.6-1 .3-1.5c-.9-1.3-1.2-1.1-1.2.6m358.2 7.4c-.3.5-.2 1.2.3 1.5s.9-.1.9-.9c0-1.7-.3-1.9-1.2-.6M346 657.3c0 .2 1.5 1.6 3.3 3.3l3.2 2.9-2.9-3.3c-2.8-3-3.6-3.7-3.6-2.9m-35 7.2c0 .2.6.8 1.3 1.4 1 .8 1.4.7 1.4-.2 0-.8-.6-1.4-1.4-1.4-.7 0-1.3.1-1.3.2m369.9 4.2c-1.3 1.6-1.2 1.7.4.4.9-.7 1.7-1.5 1.7-1.7 0-.8-.8-.3-2.1 1.3m39.7 1.5c.3.5 1 .6 1.5.3 1.3-.9 1.1-1.2-.6-1.2-.8 0-1.2.4-.9.9m25.9 12.7c-.3.5-.2 1.2.3 1.5s.9-.1.9-.9c0-1.7-.3-1.9-1.2-.6m-404.9 10.9c-.3.5.1.9.9.9s1.2-.4.9-.9c-.3-.4-.7-.8-.9-.8s-.6.4-.9.8m-71 11.1c-.3.4.3 1.6 1.3 2.5 1.4 1.3 1.8 1.4 1.8.2 0-.8-.5-1.4-1.1-1.3s-1-.4-.8-1.1c.2-1.4-.4-1.6-1.2-.3m442 11.9c-.3.5.1.9.9.9s1.2-.4.9-.9c-.3-.4-.7-.8-.9-.8s-.6.4-.9.8m43.9 1.2c-.4.6-1 .8-1.4.5-.5-.2-1.9.8-3.2 2.3l-2.4 2.7 2.6-2.3c1.4-1.2 2.8-2 3.1-1.7s1.1-.1 1.8-1c.7-.8 1-1.5.7-1.5s-.9.5-1.2 1m33.1 3.8c-.3.5.1.9.9.9s1.2-.4.9-.9c-.3-.4-.7-.8-.9-.8s-.6.4-.9.8m-9.7 10.9-2.4 2.8 2.8-2.4c1.5-1.4 2.7-2.6 2.7-2.8 0-.8-.8-.1-3.1 2.4m-480.6.8c0 .8.4 1.2.9.9s.6-1 .3-1.5c-.9-1.3-1.2-1.1-1.2.6m-42 2c0 .8.3 1.3.5 1 .3-.3 1.2.1 2.1.7 1 .8.9.4-.3-1-2.3-2.6-2.3-2.6-2.3-.7m49.3 4.3c-.3.5.1.9.9.9s1.2-.4.9-.9c-.3-.4-.7-.8-.9-.8s-.6.4-.9.8m-40.6 3.7c1.3 1.4 2.6 2.5 2.8 2.5.3 0-.5-1.1-1.8-2.5s-2.6-2.5-2.8-2.5c-.3 0 .5 1.1 1.8 2.5m500.1 2.9c-.9 1.1-.9 1.6.1 2.2.7.4 1 .4.6-.1-.4-.4-.3-1.4.3-2.2.6-.7.9-1.3.6-1.3-.2 0-1 .6-1.6 1.4m-498.1 1c0 .2.8 1 1.8 1.7 1.5 1.3 1.6 1.2.3-.4s-2.1-2.1-2.1-1.3m10.6 9.8c.3.5 1 .6 1.5.3 1.3-.9 1.1-1.2-.6-1.2-.8 0-1.2.4-.9.9m469 5.6c-.3.5.1.9.9.9s1.2-.4.9-.9c-.3-.4-.7-.8-.9-.8s-.6.4-.9.8m-456 5.4c.3.5 1 .6 1.5.3 1.3-.9 1.1-1.2-.6-1.2-.8 0-1.2.4-.9.9m447 3c.3.5 1 .6 1.5.3 1.3-.9 1.1-1.2-.6-1.2-.8 0-1.2.4-.9.9m-233.3 74.5c.9.2 2.5.2 3.5 0 .9-.3.1-.5-1.8-.5s-2.7.2-1.7.5");
        dg.appendChild(p);}
        const g2=document.createElementNS(NS,"g");
        g2.setAttribute("fill","#fff");
        g2.setAttribute("stroke-width","0");
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M472.5 140c-31 3.6-58.1 10.1-86.5 20.5-23.1 8.6-49.6 21.7-69 34.3-33 21.4-69.3 55.3-93.1 86.7-23.5 31-44.2 71.9-55.8 110.5-24 79.5-19.1 154.2 15.1 232.5 14.7 33.5 29.9 58.3 54.9 89.5 43.9 54.9 105.6 96.9 172.2 117 73.1 22.1 152.5 20.8 224.7-3.9 75.2-25.6 141.4-76.2 183.4-140 14.9-22.7 28.1-48.8 36.8-73.1 10.1-28 16.6-55.7 19.9-85 1.7-15.9 1.5-52-.6-67.5-8.5-65.8-32.5-126.9-69.3-176.4-33.8-45.4-76.7-81.8-126.2-107.3-41.8-21.5-86.8-34.6-133.5-38.8-17.4-1.6-55.7-1.1-73 1m74.1.5c29.9 2.8 53.4 7.8 80.7 16.9 22.3 7.5 52.1 20.9 66.9 30.3 2.1 1.2 4 2.3 4.3 2.3 1.2 0 28.3 18.5 36.5 24.9 31.2 24.2 59.3 54.8 81.4 88.6 24.8 37.8 44.9 88.4 53 133.5 3.1 17.4 3.6 20.8 4.7 33 1.6 17.9.6 54.8-2.1 72.5-5.8 38.6-16.5 72.5-33.8 107-15.9 32-31 53.8-56.1 81.4-7.9 8.6-30.4 29.7-37 34.5-1.5 1.2-7.3 5.5-12.7 9.6-13.4 10-29.1 19.8-45.9 28.6-35.5 18.5-62.5 27.7-107 36.4-21.1 4.1-62.1 6.6-83.7 5-92.8-6.6-168.4-40.7-232.8-105-45.9-45.8-82.6-110.2-98.5-173-7.1-27.9-9.7-48.8-9.7-77 .1-55.6 11.7-103.1 37.6-154.9 25.2-50.3 59.9-91.1 108.3-127.2 48.1-36.1 112.2-60.7 173.8-66.8 6.1-.6 12.4-1.3 14-1.5 8.6-.9 44.1-.4 58.1.9");
        g2.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M483.5 166.5c-65.5 6.3-128.5 31.6-178.8 71.8-20.9 16.6-51.3 49.3-65.4 70.2-27.3 40.5-45.4 87.1-53 136.2-2.1 13.9-2.5 64-.5 78.3 7.8 56.5 27.7 106.8 60.2 152 16.6 23.1 40.7 48.8 63.2 67.6 40.9 34.2 96.7 60 150.8 69.8 24.6 4.4 33.1 5.1 61.5 5 29.4 0 40.4-1.1 66.8-6.5 33.4-6.8 70.3-21.3 100.2-39.2 62.1-37.2 109.2-92 135.2-157.2 25-62.6 29.4-129.4 12.8-195.5-15.5-62.1-46.1-115.9-91.3-160.7-24.2-24-49.3-41.8-81.4-57.8-35.7-17.8-63.2-26.3-104.3-32.5-18.2-2.8-55.8-3.5-76-1.5M557 169c28.8 3.9 48.6 8.8 73.4 18.1 70.1 26.5 122.5 69.4 162.7 133.2 47.3 75 63.2 165.2 43.8 248-21.7 92.8-84.1 171-170.8 214.1-85.4 42.5-182.2 45.6-273.1 8.9-31.6-12.8-66-34.1-91.2-56.7-36.6-32.7-64.2-68.7-84.3-110.1-49.8-102.6-41.8-217.9 21.7-313.3 29.9-44.8 73.1-83 121.7-107.6 43.1-21.8 85.4-33.5 134.1-37 12.6-.9 47.1.4 62 2.4");
        g2.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M500.5 194.7c-62 3-120.3 23.5-168.5 59.1-59.2 43.7-98 104-112.4 174.7-7.3 35.9-7.4 78-.1 115.2 4.9 25.6 17.7 59.2 31.7 83.8 18.6 32.5 41 60.9 65.6 83.1 51.2 46.3 112.3 72.1 183.4 77.4 30.9 2.3 74.4-2.8 106.5-12.4 54.7-16.5 105.8-49.5 140.2-90.6 21.3-25.4 38.1-53.1 48.5-80 2-5.2 4.1-10.4 4.6-11.5 4-9.1 11.3-37.9 13.4-53 4.9-36.1 4.2-63.7-2.6-98.5-2.9-15.3-9.9-38.6-16.1-54-24.8-61.8-59.6-106.4-109.6-140.9-46.5-32.1-94.6-48.6-152.3-52.1-14.8-.9-19.4-1-32.3-.3m48.5 2.8c46.2 5.3 84.3 18.4 122.6 42.3 52.4 32.5 90.8 77.1 116 134.8 18.1 41.4 26 75.4 27.1 116 1.3 46.8-7.3 87.3-27.2 129.1-15.5 32.4-32.9 57-58 81.9-55.4 55-130.8 85.4-211.5 85.4-32.7 0-62.1-4.7-94.9-15.2C351.6 749 287.3 693.1 248.7 620c-31.7-59.9-41.3-126.9-27.6-192.7C232.4 372.7 259 323.9 299 284c48-47.8 112.3-79 177-85.9 5.8-.6 11.9-1.3 13.5-1.5 9.4-1 47.4-.4 59.5.9");
        g2.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M480 224.4c-50.8 6.6-98.2 26.4-137.5 57.5-20.9 16.5-46 44.5-60.4 67.3-25 39.7-37.4 81-38.8 129-.8 27.8 2.1 51 9.7 78.3 8.4 29.9 25.4 64.2 44.7 90.3 12.9 17.5 33.5 39.2 49.8 52.6 15.4 12.7 44 29.6 64.7 38.4 35.2 15 84.4 23.1 122.8 20.2 42-3.1 77.9-13 113.5-31.4 25-12.9 42.9-25.9 62-45.1 23.3-23.2 38.6-45.4 52-75.3 21.1-46.7 27.8-91 21.5-141.6-5.2-42.3-21.8-86.2-46.6-123.8-35-52.9-90.9-93-151.8-108.8-25.7-6.7-37.4-8.1-68.1-8.5-18.8-.3-30.2 0-37.5.9m75.5 2.6c48.9 7.4 90.3 25.8 129.5 57.4 15 12.1 34.8 33.4 46.6 50.1 44.9 63.5 63 144 48.3 214.9-4.8 23-10.5 39.4-21.8 63.1-19.7 40.9-48.3 73.9-86.1 99-28.9 19.3-62.2 32.8-99.5 40.4-30.5 6.3-68.4 6.8-100.5 1.5-38.2-6.3-71.3-19.3-103.5-40.6-17-11.3-26.9-19.3-41.1-33.6-62.1-62.4-91.9-146.4-81-228 6.9-51.2 30.1-100.5 65.1-138.1 8.1-8.8 24.6-24.6 30.5-29.3 41.6-33.3 90.6-53.1 144.7-58.7 15.1-1.6 52.9-.6 68.8 1.9");
        g2.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M491.6 252c-68.7 7.9-126.6 38.7-166.9 88.9-23.9 29.6-39.9 63.5-47.2 99.6-7.3 36.4-5.8 74.8 4.5 109.9 21.7 74.4 78.8 137.2 148.3 163.2 14.3 5.4 34 10.6 49.7 13.1 17.8 2.8 61.8 2.6 79.2-.5 59.5-10.3 104.8-35.4 143-79.3 15.5-17.8 33.9-50.9 42.2-76 21.7-65.5 12-142-26-205.1-19-31.6-48.1-60.9-78.1-78.8-12.9-7.7-30.9-16.9-37.8-19.3-25.2-8.7-33.3-11-50-13.8-13.7-2.3-47.8-3.4-60.9-1.9m40.2 1.1c38.7 2.8 71.1 13.1 105.6 33.6 56.1 33.3 95.8 89.8 111 157.8 4.1 18.3 5.6 33.1 5.6 55.7 0 23.5-1.4 35-6.5 55.3-4.4 17.6-9.3 30.2-18.1 47.4-12.9 24.9-23.9 39.9-43.4 59.1-34.9 34.5-77.4 55.1-131.1 63.7-17.9 2.8-57.7 2.5-76.3-.6C417.9 715 369 686.6 330 638.8c-24.3-29.9-42.5-66.9-50.4-102.8-3.6-16.2-4.5-23.3-5.2-41-1-24.5 1.6-46.7 8.3-70.5 15.3-54.3 48.9-99.7 98.3-132.9 13-8.8 38.4-20.9 55.5-26.5 21.1-6.9 35-9.6 65.5-12.4 7.6-.8 15.8-.7 29.8.4");
        g2.appendChild(p);}
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M500.5 281.1c-64 4.4-118.1 33.4-155 83-40.9 54.9-53.6 121.5-34.8 181.4 12.8 40.7 41 82.5 72.8 108.2 31.7 25.5 64.2 39.8 105 46 13.3 2 45.2 2.3 57 .5 63.3-9.7 114.8-41 147.4-89.6 6.9-10.2 18.1-33.4 22.5-46.6 14-42.1 13.3-93.9-2-139-7.9-23.4-24.4-52.6-40.6-71.7-21.7-25.7-53.2-47.8-85.1-59.7-26.4-9.9-58.5-14.5-87.2-12.5M537 283c35.5 3.7 71.3 17.5 99.3 38.3 39.8 29.7 66.4 69 80.1 118.3 4.7 16.9 6.6 30.6 7.3 53.4 1 33.6-4.3 61.4-17.3 90.3-24.4 54.1-74.3 95-134.3 110.2-58.9 14.9-121.6 4.9-169.1-27.1-30.7-20.6-51.5-43.1-71.9-77.7-16.7-28.3-28.1-68.5-28.1-99.2 0-9.5 2.5-30.2 5.1-42.2 10.4-48.5 43-97.9 83.4-126.9 24.7-17.7 52-29.2 82.9-34.9 17.7-3.3 44.6-4.4 62.6-2.5");
        g2.appendChild(p);}
        dg.appendChild(g2);
        s.appendChild(dg);
        const ag=document.createElementNS(NS,"g");
        ag.setAttribute("class","ytune-arm-g");
        {const p=document.createElementNS(NS,"path");
        p.setAttribute("d","M778.2 187c-9.7 2.6-18.5 6.9-25.7 12.7-29.6 24-34 65.5-10 93.8 3 3.5 5.5 6.7 5.5 6.9 0 .3-4.7 9-10.5 19.3-6 10.7-10.5 19.9-10.5 21.4s-2.4 9.6-5.4 18c-2.9 8.5-12.4 36.8-21 62.9-30.3 91.5-31.8 95.5-43.3 111.7-12 17-30.4 32.1-55.5 45.7-4.7 2.6-28.8 14.4-53.5 26.5l-45.1 21.8-2.5-2.5c-1.4-1.3-3.6-2.4-4.8-2.4-1.3 0-12.3 4.9-24.4 10.8-19.3 9.5-22 11.2-22.3 13.4-.3 2.5 12.1 28.2 19.4 40.2 5.5 9.1 5.3 9.1 22.2.2 25.8-13.5 27.9-14.8 28.9-19.1 1-4.4-.8-3.3 44.8-27 31-16.1 52.2-27.7 63.5-34.7 31.9-19.7 57.5-47.5 69.3-75.1 4-9.5 9.5-26.7 21.2-66.5 4.8-16.2 10.4-35.1 12.5-42 19.4-63.1 20.4-66.2 23.6-70.3 1.7-2.3 6.9-11 11.4-19.2 11.9-21.7 10.1-19.6 15.2-17.7 6.2 2.2 24.5 2.2 32.1-.1 33.3-10.1 53.3-42.1 47.3-75.5-2.6-14.7-12-30.7-23.5-39.9-6.9-5.5-19.6-11.7-28.2-13.8-8.7-2-21.8-1.8-30.7.5m32.4 10.8c9.4 2.9 16.2 7.1 23.5 14.2 7.4 7.3 13.1 16.7 15.3 25.5 2 7.6 2 20.4.1 27.9-7 26.8-34.1 45.1-60.2 40.7-4-.7-7.3-1.6-7.3-2s1.5-3.1 3.3-6.1l3.2-5.5 7-.1c21.8-.5 36.9-13.4 40.5-34.7 3.6-20.9-10.1-41.7-31.4-47.3-18.3-4.9-39.2 4.7-47.7 22.1-6.1 12.6-6 25.1.6 38 1.9 3.8 3.5 7.3 3.5 7.7 0 .7-5.6 10.5-6.8 11.8-.8 1-7.9-7.9-11-13.8-17.1-32 3.4-71.8 41.3-80.2 5.8-1.3 19-.4 26.1 1.8m-.4 19.5c13.1 6.4 20.7 17.4 21.6 31.1 1.3 21.1-12.7 37.9-32.7 39.4-5.9.4-7.1.2-7.1-1 0-1.1 1.5-1.7 5.3-2.1 19.7-2 31.7-16 30.5-35.6-.9-15.5-10.3-26.7-25.6-30.6-14.8-3.8-29.8 2.9-37.3 16.7-3.2 5.9-3.4 6.9-3.4 15.8 0 8.5.3 10 2.8 14.4 2.1 3.8 2.5 5.2 1.6 6.3-1 1.1-1.6.7-3.5-2-3.6-5.4-5.7-14.1-5.2-21.7 1.1-14.5 10.2-26.4 24.4-32 7.5-3 21.2-2.3 28.6 1.3m-24.8 51.9c.9 1.2 1.6 3.1 1.6 4.2 0 1-3.3 6.9-7.4 13-4 6.1-12.8 20.9-19.6 32.8-6.7 12-12.5 21.8-12.9 21.8-.3 0-1.7-.9-3.1-2s-3.1-2-3.7-2c-.7 0-1.3-.4-1.3-.8s4.9-9.3 10.8-19.7c5.9-10.5 14.1-25.1 18.2-32.5 4-7.4 8-14.3 8.8-15.3 2.1-2.4 6.7-2.2 8.6.5m-45 78.7c1.7 1.9 1.5 3-7.3 29.3-8.5 25.6-15.9 49.3-33.6 107.3-6.8 22-13.7 41.1-18 49.6-4.5 8.8-14.9 22.3-24 31.4-19.2 19.1-32.3 27.2-94.8 58.6l-45.8 23-1.9-2.3c-2.1-2.6-3.5-7.2-2.5-8.7.8-1.4 15.1-8.7 52.5-26.9 59.3-28.9 78.2-41.5 95.5-63.6 12.7-16.3 18.4-28.9 32.6-72.2 5-15.3 16.5-50.3 25.6-77.7 17.5-52.6 17.4-52.5 21.7-47.8M499 637.1c.7 1.1.9 2.3.5 2.7-.3.3-8.1 4.3-17.3 8.8l-16.7 8.3-1.8-2.1c-1-1.2-1.6-2.6-1.4-3.1.4-1.4 30.7-16.4 33.4-16.6 1.3 0 2.7.7 3.3 2m9 13.8c.5 1.1 1 2.3 1 2.8s-8.1 4.9-18.1 9.8c-17.5 8.7-18.1 8.9-19.5 7-.8-1.1-1.3-2.6-1.2-3.3.4-1.6 31.5-18 34.5-18.1 1.2-.1 2.7.8 3.3 1.8m-82-5.8c-21.2 9.8-29.3 13.7-55.5 26.8-27.2 13.5-70.3 36-78.8 41.1-3.7 2.2-6.7 6.4-6.7 9.5 0 4.6 23.8 45.9 30.6 53.2 3.7 3.9 7.9 4.3 14.1 1.6 7.8-3.6 52.3-28.7 54.7-31 3-2.8 3.6-2.9 3.6-.5s-1.8 3.9-10.4 9.2c-13.3 8-55.9 31-57.6 31-2.4 0-4.7-2.8-14.9-18.3-5-7.5-9.7-13.7-10.4-13.7-3.2.2-17.7 10.5-17.7 12.7s8.5 18.3 11.8 22.2l2.9 3.5 6.5-3.7c3.7-2 6.8-3.7 7-3.7.3 0 1.9 2.2 3.7 4.9 4 6.3 8.5 10.1 11.8 10.1 2.7 0 23.5-10.5 51.9-26.1 8.9-4.9 16.6-8.9 17.1-8.9s6.3 8 12.8 17.7c12.7 19.2 15.1 22.3 17.3 22.3s7.2-4 7.2-5.7c0-1.4-3.8-7.6-25.7-41-2.9-4.4-5.3-8.8-5.3-9.6 0-1.6 5.2-5.9 9.5-7.9 2.5-1.1 2.9-1 4.7 1.5 3.6 5 4.5 4.6 32.5-14 22.4-14.9 30.3-21.3 30.3-24.6 0-2.5-2.7-7.3-7.5-13.2-2.9-3.7-6.8-10.6-19.6-35-7.9-15-8.9-15.5-19.9-10.4m34.4 50.1c-.2.7-9.6 6.7-20.7 13.3-25.9 15.3-49.6 29.4-52.4 31.3-2.1 1.3-2.3 1.3-2.3-.5 0-2.4 5.7-6.2 36.1-23.9 24.4-14.3 37.4-21.4 38.9-21.4.5 0 .7.6.4 1.2m-108.5 4c1.4 2.5 1.4 2.9-.5 5.4-2.8 3.8-8.8 6.8-11.9 6-3.8-1-5.2-4.8-3.1-8.1 4.3-6.5 12.7-8.3 15.5-3.3M327 714c4 4 1.8 8.6-5.8 12.4-4.6 2.3-8.3 1.7-10.2-1.7-1.3-2.4-1.3-3 0-5.4 1.8-3.2 8.4-7.3 11.8-7.3 1.2 0 3.1.9 4.2 2m43.2 16.7c2.9 2.6 2.3 6.7-1.7 10.2-5.8 5.1-10.9 5.5-13.4 1-2-3.6-.1-7.9 4.7-10.7 4.5-2.6 7.9-2.8 10.4-.5M344 748c2.5 2.5 2.5 4.1-.1 7.4-7.2 9.1-21.8 6-15.3-3.3 4-5.6 11.8-7.7 15.4-4.1m47.4 9.7c-2.8 3.1-3.6 3.5-4.3 2.4s-.1-2.1 2.2-3.7c4.9-3.5 6-2.8 2.1 1.3m-90.4 8.2c0 2.1-9.2 8.1-12.5 8.1-2.5 0-1.7-2 1.6-3.8 1.8-.9 4.3-2.7 5.8-3.8 2.3-2 5.1-2.2 5.1-.5");
        ag.appendChild(p);}
        s.appendChild(ag);
        playerBtn.appendChild(s);
        })();

        // ytune mini menu button + modal — filled circle with brand icon
        const menuBtn = document.createElement('button');
        menuBtn.id = 'ytune-menu-btn';
        menuBtn.title = 'ytune';
        menuBtn.style.cssText = [
            'position:relative', 'display:flex', 'align-items:center',
            'justify-content:center', 'width:36px', 'height:36px',
            'border-radius:50%', 'background:#181818', 'border:none',
            'cursor:pointer', 'color:#fff',
            'transition:transform 0.15s,background 0.2s',
            'flex-shrink:0', 'padding:0',
        ].join(';');
        menuBtn.addEventListener('mouseenter', () => { menuBtn.style.transform = 'scale(1.1)'; });
        menuBtn.addEventListener('mouseleave', () => { menuBtn.style.transform = 'scale(1)'; });
        (function() {
            const NS = 'http://www.w3.org/2000/svg';
            const svg = document.createElementNS(NS, 'svg');
            svg.setAttribute('viewBox', '0 0 1024 1024');
            svg.setAttribute('width', '26');
            svg.setAttribute('height', '26');
            const g = document.createElementNS(NS, 'g');
            g.setAttribute('fill', 'currentColor');
            g.setAttribute('stroke-width', '0');
            const paths = [
                'M478.6 20c-40.1 2.8-82 10.9-119.2 23-97.9 32-181.4 91.2-244 173-33.7 44-62.2 99.7-78.8 153.9-8.3 27.1-16.7 68.8-19.7 98.6-1.7 15.6-1.7 72.1-.1 89 4.6 47.7 18.8 102 39.5 150 12.6 29.3 37.4 71.6 57.8 98.6 38.5 51 90.8 97.7 145.5 129.9 38.1 22.5 83.9 42.2 118.5 51 4.6 1.2 12.2 3.3 16.9 4.6 11.1 3.2 43.7 8.8 67 11.5 27.2 3.2 76.8 3.2 103 0 34.6-4.1 54.8-8.1 83.5-16.4C741.4 959.8 821.3 909 887.1 835c50.5-56.8 87.5-126.7 107.3-202.5 5.6-21.6 6.9-27.9 10-50 3.9-27.5 4.7-39.6 4.7-69 0-52.3-6-93.2-20.8-142.9-16.8-56.7-46.6-113.8-84.2-161.6-30.8-39-71.8-77.3-111.6-104.1-70.6-47.4-151.1-76.3-234-83.9-16.2-1.5-64-2.1-79.9-1M561 36.5c45.2 5 88.8 15.7 127.5 31.2 112.1 45 201.7 127 255 233.3 8.6 17.2 20.7 46.9 26.8 66 7.2 22.6 9.9 32.7 14.1 53.5 16.4 80.3 11.6 158.8-14.6 238.4-27.2 82.7-77 156.9-144.3 214.6-66 56.7-150 96.1-235.5 110.4-44.7 7.5-100 7.9-147.5 1-14.9-2.1-46.6-9.1-63-13.8-57.1-16.4-118.8-48.3-166-86C140.9 827.3 86.2 749.8 56 662.5c-23.5-68.4-30.6-142.4-20.4-214 2.5-17.7 6.2-37.3 9-47.5 12-43.7 19.6-64.5 36.4-98.5 20.3-41.3 40.4-71.2 71.6-106.5 62.6-70.9 150.7-124.3 243.2-147.4 27.6-6.9 67-12.9 92.7-14 15.7-.7 59.2.5 72.5 1.9',
                'M385.3 263.1c-10.9 4.2-19.3 14.9-21.2 26.9-1.6 9.4-1.5 403 0 410.2 2.3 10.6 7.9 17.6 18.3 22.8 5 2.6 7.1 3 13.7 3 4.7 0 10-.7 13.6-1.8 3.5-1.1 16.9-8.6 33.8-18.9 30.5-18.5 38.9-23.5 54-32.5 5.5-3.3 11.6-7 13.5-8.2 5.5-3.6 25.1-15.4 41.5-25.1 8.3-4.8 20-11.8 26-15.5 6.1-3.7 24.1-14.6 40-24.2 16-9.6 38.9-23.5 51-30.8s33.3-20 47-28.3c13.8-8.2 26.7-16.6 28.8-18.6s5.5-7 7.5-11c3.5-7.1 3.7-8.1 3.7-17 0-8.7-.3-10.2-3.2-16.3-4.4-9.3-10.4-14.9-26.3-24.4-7.4-4.5-24.1-14.6-37-22.4s-28.9-17.5-35.5-21.5c-6.6-3.9-20.3-12.3-30.5-18.5s-22.8-13.9-28-17c-5.2-3.2-16.2-9.9-24.5-15-8.2-5.1-29.6-18.1-47.5-28.9-45.5-27.6-88.4-53.7-95.5-58.2-3.3-2.1-8.9-5.3-12.5-7.1-5.8-3-7.5-3.3-16-3.5-7.6-.2-10.5.1-14.7 1.8',
                'M249.5 367.5c-9.9 3-29.9 8.8-44.5 13-53 15.3-54.3 15.7-56.9 22.8-.7 1.9-1.1 24.1-1.1 67.4v64.5l-2.7-.6c-6.5-1.4-17.2-1.8-22.3-.7-22.4 4.8-40.9 27.3-37.1 45.1 2.2 10.4 7.7 17.1 17.6 21.6 19.9 9.1 48.6-2.7 58.8-24.3l3.2-6.8.5-62.2c.4-53.6.7-62.4 2-63.7.8-.8 6.7-2.8 13-4.6s21.4-6.1 33.5-9.7c52-15.2 46.6-13.9 48.1-11.8 1.1 1.5 1.4 9.4 1.4 40.9 0 21.5-.3 40.8-.6 42.9-.7 3.7-.8 3.8-3.8 3-1.7-.4-7.6-.8-13.1-.7-11.4.1-16.7 1.8-26.4 8.3-7.4 5.1-11.1 9.4-15.2 17.7-8.5 17.2-.9 35.9 16.9 41.4 8.3 2.6 16.3 2.5 24.8-.1 10-3.1 14.1-5.4 21-12s10.7-13.5 12.4-22.4c.8-4.3 1-29.2.8-87.2-.3-80.6-.3-81.2-2.4-84-3.2-4.3-7.3-3.9-27.9 2.2M809 402.9c-.5 1.1-1 8.2-1 16 0 11.4.3 14.2 1.6 15.5 1.3 1.4 5 1.6 24.4 1.6 28.2 0 26 1.5 26-18.2 0-11.1-.3-14-1.6-15.2-1.3-1.4-5-1.6-24.9-1.6-22.1 0-23.5.1-24.5 1.9m.6 47.7c-1.4 1.3-1.6 5.5-1.6 29.9 0 28.2 0 28.4 2.2 29.9 1.9 1.3 6 1.6 24.4 1.6 20.1 0 22.2-.2 23.7-1.8s1.7-5.2 1.7-29.3c0-23.2-.2-27.8-1.6-29.7-1.5-2.2-1.9-2.2-24.4-2.2-19.4 0-23.1.2-24.4 1.6m72.4 30.3c-.5 1.1-1 7.5-1 14.3 0 18.1-2 16.8 26 16.8 19.4 0 23.1-.2 24.4-1.6 1.3-1.2 1.6-4.1 1.6-14.9s-.3-13.7-1.6-14.9c-1.3-1.4-5-1.6-24.9-1.6-22.1 0-23.5.1-24.5 1.9m-72.8 44.3c-.8.8-1.2 5.6-1.2 14.8s.4 14 1.2 14.8c.9.9 7.7 1.2 24.4 1.2 19.8 0 23.5-.2 24.8-1.6 1.3-1.2 1.6-4 1.6-14 0-6.9-.5-13.4-1-14.5-1-1.8-2.4-1.9-24.8-1.9-17.2 0-24.1.3-25 1.2m72.8.7c-.5 1.1-1 7.6-1 14.4 0 10.7.2 12.6 1.8 14 1.6 1.5 4.9 1.7 25 1.7 21.8 0 23.2-.1 24.2-1.9.5-1.1 1-7.4 1-14.1s-.5-13-1-14.1c-1-1.8-2.4-1.9-25-1.9s-24 .1-25 1.9m-144.4 43.7c-1.3 1.2-1.6 4.1-1.6 15.4 0 12.7.2 14 1.9 15 1.2.6 10.7 1 24 1 26.9 0 25.1 1.2 25.1-16.5s1.9-16.5-25.5-16.5c-19 0-22.6.2-23.9 1.6m72.2-.5c-1.6.9-1.8 2.7-1.8 15.4 0 13.2.2 14.5 1.9 15.5 1.2.6 10.8 1 24.4 1 20.4 0 22.5-.2 24-1.8 1.4-1.5 1.7-4.1 1.7-14.3 0-18.2 2-16.9-25.8-16.9-13.6 0-23.4.5-24.4 1.1m72.8 1.1c-1.3 1.7-1.6 5.1-1.6 14.9 0 10.9.2 12.8 1.8 14.2 1.6 1.5 4.8 1.7 24.5 1.7 19.2 0 22.8-.2 24.1-1.6 1.3-1.2 1.6-4 1.6-14.2 0-18.6 2-17.2-26-17.2-22.5 0-22.9 0-24.4 2.2m-145.4 44c-.8.8-1.2 5.6-1.2 14.9 0 11.1.3 13.9 1.6 14.9 2.1 1.8 46 1.8 47.7.1.8-.8 1.3-5.9 1.5-13.7.3-10.2.1-13-1.2-14.9L784 613h-22.8c-16.4 0-23.1.3-24 1.2m71.8.7c-.5 1.1-1 7.8-1 14.9 0 10.8.3 13.2 1.7 14.3 1.3 1.1 6.5 1.4 24.6 1.4 21.4 0 23-.2 24.3-2 2.1-2.9 2-26.8-.2-28.9-1.3-1.4-5-1.6-24.9-1.6-22.1 0-23.5.1-24.5 1.9m73.7-.1c-2.3 2.5-2.5 25.7-.3 28.8 1.4 1.8 2.8 1.9 24.6 1.9s23.2-.1 24.6-1.9c1.7-2.4 2-25.6.4-28.7-1-1.8-2.4-1.9-24.4-1.9-21.3 0-23.4.2-24.9 1.8'
            ];
            paths.forEach(d => {
                const p = document.createElementNS(NS, 'path');
                p.setAttribute('d', d);
                g.appendChild(p);
            });
            svg.appendChild(g);
            menuBtn.appendChild(svg);
        })();

        // Mini modal
        const modal = document.createElement('div');
        modal.id = 'ytune-modal';
        modal.style.cssText = [
            'position:fixed', 'z-index:9999',
            'background:#1e1e1e', 'border:1px solid rgba(255,255,255,0.1)',
            'border-radius:14px', 'padding:16px', 'width:220px',
            'box-shadow:0 8px 32px rgba(0,0,0,0.6)',
            'display:none', 'flex-direction:column', 'gap:8px',
            'font-family:Roboto,sans-serif', 'color:#fff',
        ].join(';');

        const modalTitle = document.createElement('div');
        modalTitle.id = 'ytune-modal-title';
        modalTitle.style.cssText = 'font-size:15px;font-weight:600;padding-bottom:8px;border-bottom:1px solid rgba(255,255,255,0.08);margin-bottom:4px;letter-spacing:0.3px';
        modalTitle.textContent = 'ytune';

        function makeModalItem(icon, label, sublabel) {
            const NS = 'http://www.w3.org/2000/svg';
            const row = document.createElement('div');
            row.style.cssText = 'display:flex;align-items:center;gap:12px;padding:8px;border-radius:8px;cursor:pointer;transition:background 0.15s';
            row.addEventListener('mouseenter', () => row.style.background = 'rgba(255,255,255,0.08)');
            row.addEventListener('mouseleave', () => row.style.background = 'transparent');
            const svg = document.createElementNS(NS, 'svg');
            svg.setAttribute('viewBox', '0 0 24 24');
            svg.setAttribute('width', '18'); svg.setAttribute('height', '18');
            svg.setAttribute('fill', 'rgba(255,255,255,0.5)');
            const p = document.createElementNS(NS, 'path');
            p.setAttribute('d', icon);
            svg.appendChild(p);
            const text = document.createElement('div');
            const name = document.createElement('div');
            name.style.cssText = 'font-size:13px;color:#fff';
            name.textContent = label;
            text.appendChild(name);
            if (sublabel) {
                const sub = document.createElement('div');
                sub.style.cssText = 'font-size:11px;color:rgba(255,255,255,0.4);margin-top:1px';
                sub.textContent = sublabel;
                text.appendChild(sub);
            }
            row.appendChild(svg);
            row.appendChild(text);
            return row;
        }

        const itemPlayer = makeModalItem(
            'M12 3v10.55A4 4 0 1014 17V7h4V3h-6zm-2 16a2 2 0 110-4 2 2 0 010 4z',
            'Open player',
            'Floating widget'
        );
        itemPlayer.addEventListener('click', () => {
            window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
                event: 'ytune-toggle-popup', payload: {},
            }).catch(() => {});
            modal.style.display = 'none';
        });

        const itemScrobble = makeModalItem(
            'M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 14H9V8h2v8zm4 0h-2V8h2v8z',
            'Last.fm / ListenBrainz',
            'Coming soon'
        );

        // Dynamic ytune account item — shows login circles or logged-in state
        const _mAcctSubEl = document.createElement('div');
        _mAcctSubEl.style.cssText = 'font-size:11px;color:rgba(255,255,255,0.4);margin-top:1px';
        _mAcctSubEl.textContent = 'Login to scrobble';

        const _mAcctLoginRow = document.createElement('div');
        _mAcctLoginRow.style.cssText = 'display:flex;gap:10px;align-items:center;padding:6px 8px 10px 38px';

        const _mAcctUserRow = document.createElement('div');
        _mAcctUserRow.style.cssText = 'display:none;align-items:center;gap:8px;padding:4px 8px 10px 38px';

        const _mAcctAvatarEl = document.createElement('div');
        _mAcctAvatarEl.style.cssText = 'width:26px;height:26px;border-radius:50%;background:#5865F2;display:flex;align-items:center;justify-content:center;font-size:12px;font-weight:700;color:#fff;flex-shrink:0;background-size:cover;background-position:center';

        const _mAcctNameEl = document.createElement('span');
        _mAcctNameEl.style.cssText = 'font-size:12px;color:rgba(255,255,255,0.7);flex:1';

        const itemAccount = (function() {
            const _aNS = 'http://www.w3.org/2000/svg';
            const wrap = document.createElement('div');
            wrap.style.cssText = 'display:flex;flex-direction:column;border-radius:8px;overflow:hidden';

            const hdr = document.createElement('div');
            hdr.style.cssText = 'display:flex;align-items:center;gap:12px;padding:8px;cursor:default';
            const asvg = document.createElementNS(_aNS, 'svg');
            asvg.setAttribute('viewBox', '0 0 24 24');
            asvg.setAttribute('width', '18'); asvg.setAttribute('height', '18');
            asvg.setAttribute('fill', 'rgba(255,255,255,0.5)');
            const ap = document.createElementNS(_aNS, 'path');
            ap.setAttribute('d', 'M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z');
            asvg.appendChild(ap); hdr.appendChild(asvg);
            const atxt = document.createElement('div');
            const albl = document.createElement('div');
            albl.style.cssText = 'font-size:13px;color:#fff'; albl.textContent = 'ytune account';
            atxt.appendChild(albl); atxt.appendChild(_mAcctSubEl);
            hdr.appendChild(atxt); wrap.appendChild(hdr);

            const aHint = document.createElement('span');
            aHint.style.cssText = 'font-size:11px;color:rgba(255,255,255,0.3)';
            aHint.textContent = 'Sign in';
            const mDiscBtn = makeLoginCircle('#5865F2', null, 'Discord', [
                ['M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057c.002.022.015.04.034.052a19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03z', '#fff'],
            ]);
            mDiscBtn.style.width = '38px'; mDiscBtn.style.height = '38px';
            mDiscBtn.addEventListener('click', function() {
                window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'ytune-open-url', payload: 'https://ytune.asktome.com.br/auth/discord' }).catch(function() {});
                modal.style.display = 'none';
            });
            const mGoogBtn = makeLoginCircle('#fff', '0 2px 6px rgba(0,0,0,0.3)', 'Google', [
                ['M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z', '#4285F4'],
                ['M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z', '#34A853'],
                ['M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l3.66-2.84z', '#FBBC05'],
                ['M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z', '#EA4335'],
            ]);
            mGoogBtn.style.width = '38px'; mGoogBtn.style.height = '38px';
            mGoogBtn.addEventListener('click', function() {
                window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'ytune-open-url', payload: 'https://ytune.asktome.com.br/auth/google' }).catch(function() {});
                modal.style.display = 'none';
            });
            _mAcctLoginRow.appendChild(aHint);
            _mAcctLoginRow.appendChild(mDiscBtn);
            _mAcctLoginRow.appendChild(mGoogBtn);
            wrap.appendChild(_mAcctLoginRow);

            const mSignOut = document.createElement('span');
            mSignOut.style.cssText = 'font-size:11px;color:rgba(255,255,255,0.35);cursor:pointer;padding:2px 8px;border-radius:10px;transition:background 0.15s';
            mSignOut.textContent = 'Sign out';
            mSignOut.addEventListener('mouseenter', function() { mSignOut.style.background = 'rgba(255,255,255,0.08)'; });
            mSignOut.addEventListener('mouseleave', function() { mSignOut.style.background = 'transparent'; });
            mSignOut.addEventListener('click', function() { localStorage.removeItem('ytune_token'); _refreshAllAccountUI(); });
            _mAcctUserRow.appendChild(_mAcctAvatarEl); _mAcctUserRow.appendChild(_mAcctNameEl); _mAcctUserRow.appendChild(mSignOut);
            wrap.appendChild(_mAcctUserRow);

            return wrap;
        })();

        // Discord presence toggle
        let _discordEnabled = true;
        const itemDiscord = makeModalItem(
            'M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057c.002.022.015.04.034.052a19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03z',
            'Discord Presence',
            'Enabled'
        );
        const discordToggle = document.createElement('div');
        discordToggle.style.cssText = 'width:32px;height:18px;border-radius:9px;background:#555;position:relative;transition:background 0.2s;flex-shrink:0;cursor:pointer';
        const discordKnob = document.createElement('div');
        discordKnob.style.cssText = 'width:14px;height:14px;border-radius:50%;background:#fff;position:absolute;top:2px;left:2px;transition:left 0.2s';
        discordToggle.appendChild(discordKnob);
        itemDiscord.appendChild(discordToggle);

        function setDiscordToggleUI(enabled) {
            _discordEnabled = enabled;
            discordToggle.style.background = enabled ? '#5865F2' : '#555';
            discordKnob.style.left = enabled ? '16px' : '2px';
            const sub = itemDiscord.querySelector('div > div:last-child');
            if (sub) sub.textContent = enabled ? 'Enabled' : 'Disabled';
        }
        window.__ytune__._setDiscordToggleUI = setDiscordToggleUI;
        setDiscordToggleUI(_discordEnabled);

        itemDiscord.addEventListener('click', (e) => {
            e.stopPropagation();
            setDiscordToggleUI(!_discordEnabled);
            window.__TAURI_INTERNALS__?.invoke('plugin:event|emit', {
                event: 'ytune-discord-toggle', payload: {},
            }).catch(() => {});
        });

        // Request real persisted state from Rust (response arrives via eval → setDiscordState)
        window.__TAURI_INTERNALS__?.invoke('plugin:event|emit', {
            event: 'ytune-discord-state-request', payload: {},
        }).catch(() => {});

        modal.appendChild(modalTitle);
        modal.appendChild(itemPlayer);
        modal.appendChild(itemDiscord);
        modal.appendChild(itemScrobble);
        modal.appendChild(itemAccount);
        document.body.appendChild(modal);
        document.body.appendChild(accountDropdown);

        function positionModal() {
            const r = menuBtn.getBoundingClientRect();
            modal.style.top  = (r.bottom + 8) + 'px';
            modal.style.right = (window.innerWidth - r.right) + 'px';
        }

        menuBtn.addEventListener('click', (e) => {
            e.stopPropagation();
            const visible = modal.style.display === 'flex';
            modal.style.display = visible ? 'none' : 'flex';
            if (!visible) { positionModal(); _refreshAllAccountUI(); }
        });

        document.addEventListener('click', (e) => {
            if (!modal.contains(e.target) && e.target !== menuBtn)
                modal.style.display = 'none';
        });

        // Style element for dynamic palette colors
        let styleEl = document.getElementById('ytune-btn-style');
        if (!styleEl) {
            styleEl = document.createElement('style');
            styleEl.id = 'ytune-btn-style';
            document.head.appendChild(styleEl);
        }
        _ytuneBtnStyle = styleEl;

        wrap.appendChild(playerBtn);
        wrap.appendChild(menuBtn);
        wrap.appendChild(bellBtn);
        wrap.appendChild(accountBtn);
        avatar.parentElement.insertBefore(wrap, avatar);

        // Apply default palette immediately; will be updated when a song loads
        window.__ytune__.updateHeaderColors(280, 60);

        // Restore logged-in state from persisted token on startup
        _refreshAllAccountUI();

        console.log('[ytune] header buttons injected');
    }

    // Expose so Rust/popup can later set notification count
    window.__ytune__.setNotifCount = function(n) {
        const badge = document.getElementById('ytune-notif-badge');
        if (!badge) return;
        badge.style.display = n > 0 ? 'block' : 'none';
    }

    window.__ytune__.setVinylPlaying = function(playing) {
        const btn = document.getElementById('ytune-player-btn');
        if (!btn) return;
        if (playing) btn.classList.add('ytune-playing');
        else btn.classList.remove('ytune-playing');
    };

    window.__ytune__.setDiscordState = function(enabled) {
        if (typeof window.__ytune__._setDiscordToggleUI === 'function')
            window.__ytune__._setDiscordToggleUI(enabled);
    };

    window.__ytune__.setAuthToken = function(token) {
        if (token) localStorage.setItem('ytune_token', token);
        else        localStorage.removeItem('ytune_token');
        if (typeof window.__ytune__._refreshAllAccountUI === 'function')
            window.__ytune__._refreshAllAccountUI();
    };

    function waitForHeaderButtons() {
        if (document.querySelector('#right-content ytmusic-settings-button')) {
            injectHeaderButtons();
            return;
        }
        const obs = new MutationObserver(() => {
            if (document.querySelector('#right-content ytmusic-settings-button')) {
                obs.disconnect();
                injectHeaderButtons();
            }
        });
        obs.observe(document.documentElement, { childList: true, subtree: true });
        setTimeout(() => obs.disconnect(), 15000);
    }

    // initialization_script runs before the DOM exists — wait for DOMContentLoaded
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', () => { waitForPlayerBar(); waitForHeaderButtons(); });
    } else {
        waitForPlayerBar();
        waitForHeaderButtons();
    }
})();
"##;
