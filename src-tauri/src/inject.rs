pub const INJECT_JS: &str = r##"
(function() {
    // initialization_script runs in every frame — skip anything that isn't the main YTM page
    if (location.hostname !== 'music.youtube.com') return;

    // Block beforeunload dialogs — YTM registers them to warn on navigation,
    // but ytune navigates programmatically and the dialog is always unwanted.
    // This runs before YTM's scripts so no listener ever gets registered.
    if (!window.__ytune_beforeunload_blocked) {
        window.__ytune_beforeunload_blocked = true;
        const _ael = EventTarget.prototype.addEventListener;
        EventTarget.prototype.addEventListener = function(type, listener, options) {
            if (type === 'beforeunload') return;
            return _ael.call(this, type, listener, options);
        };
        Object.defineProperty(window, 'onbeforeunload', {
            get() { return null; }, set(_) {}, configurable: true,
        });
    }

    if (window.__ytune_injected) return;
    window.__ytune_injected = true;

    console.log('[ytune] script active, __TAURI_INTERNALS__:', typeof window.__TAURI_INTERNALS__ !== 'undefined');

    window.__ytune__ = {
        playPause: () => document.querySelector('#play-pause-button button')?.click(),
        next:      () => document.querySelector('.next-button.ytmusic-player-bar button')?.click(),
        previous:  () => document.querySelector('.previous-button.ytmusic-player-bar button')?.click(),
        shuffle:   () => document.querySelector('.shuffle.ytmusic-player-bar button')?.click(),
        repeat:    () => document.querySelector('.repeat.ytmusic-player-bar button')?.click(),
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
    let _thumbData = { url: '', data: '', display: '' };

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
        _thumbData   = { url: '', data: '', display: '' }; // clear stale data from previous song

        function loadAndExtract(src) {
            fetch(src)
                .then(r => { if (!r.ok) throw new Error('HTTP ' + r.status); return r.blob(); })
                .then(blob => {
                    const blobUrl = URL.createObjectURL(blob);
                    const img = new Image();
                    img.onload = function() {
                        const s = Math.min(img.naturalWidth, img.naturalHeight);
                        const sx = (img.naturalWidth - s) / 2;
                        const sy = (img.naturalHeight - s) / 2;
                        // 300×300 data URI used as img src in popup (avoids 429 from localhost)
                        const DISP = 300;
                        const cd = document.createElement('canvas');
                        cd.width = cd.height = DISP;
                        cd.getContext('2d').drawImage(img, sx, sy, s, s, 0, 0, DISP, DISP);
                        const displayDataUri = cd.toDataURL('image/jpeg', 0.92);
                        // 60×60 canvas for palette extraction
                        const SIZE = 60;
                        const c = document.createElement('canvas');
                        c.width = c.height = SIZE;
                        const ctx = c.getContext('2d');
                        ctx.drawImage(img, sx, sy, s, s, 0, 0, SIZE, SIZE);
                        URL.revokeObjectURL(blobUrl);
                        _thumbData = { url: src, data: c.toDataURL('image/jpeg', 0.85), display: displayDataUri };
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
        // Scope to the queue panel — only rendered in DOM when the panel has been opened.
        // Global fallback was removed: it picks up items from unrelated DOM contexts
        // (suggestions, shelves) producing wrong track lists and broken current detection.
        const container = document.querySelector('ytmusic-player-queue #contents')
                       || document.querySelector('ytmusic-player-queue');
        if (!container) return [];
        const items = container.querySelectorAll('ytmusic-player-queue-item');
        if (items.length < 2) return [];

        const currentDomIdx = Array.from(items).findIndex(el => el.hasAttribute('selected'));

        // Primary source: ytmusic-player-queue Polymer data.
        // queueEl.data is an Array of playlist item renderers populated before lazy image
        // loading fires, so all items carry videoId and high-res album art thumbnails.
        // queueOffset anchors the array to the current DOM item via pvr.selected.
        let queueItems = null;
        let queueOffset = 0;
        try {
            const qData = document.querySelector('ytmusic-player-queue')?.data;
            if (Array.isArray(qData) && qData.length > 0) {
                queueItems = qData;
                const selIdx = qData.findIndex(qi => qi?.playlistPanelVideoRenderer?.selected);
                if (selIdx >= 0) queueOffset = selIdx;
            }
        } catch(e) {}

        const raw = Array.from(items).map((el, idx) => {
            let vid = '', polymerThumb = '';

            if (queueItems && currentDomIdx >= 0) {
                const pos = queueOffset + (idx - currentDomIdx);
                if (pos >= 0 && pos < queueItems.length) {
                    try {
                        const entry = queueItems[pos];
                        // YTM uses several renderer types for queue entries
                        const pvr = entry?.playlistPanelVideoRenderer
                                 || entry?.playlistPanelVideoWrapperRenderer?.primaryRenderer?.musicTwoRowItemRenderer
                                 || entry?.playlistPanelVideoWrapperRenderer?.primaryRenderer
                                 || (entry?.videoId ? entry : null);
                        if (pvr?.videoId) {
                            vid = pvr.videoId;
                            const thumbs = pvr.thumbnail?.thumbnails;
                            if (Array.isArray(thumbs) && thumbs.length > 0)
                                polymerThumb = thumbs[thumbs.length - 1]?.url || '';
                        }
                    } catch(e) {}
                }
            }

            // Fallback: lazy img.src for items outside queueItems range (ignore base64 placeholder)
            const lazyImg = el.querySelector('yt-img-shadow img')?.src || '';
            const thumb = polymerThumb
                       || (vid ? 'https://i.ytimg.com/vi/' + vid + '/mqdefault.jpg' : '')
                       || (lazyImg.startsWith('data:') ? '' : lazyImg);

            return {
                title:    el.querySelector('.song-title')?.textContent?.trim() || '',
                artist:   el.querySelector('.byline')?.textContent?.trim() || '',
                thumb,
                current:  el.hasAttribute('selected'),
                domIndex: idx,
            };
        });
        // Deduplicate by title (case-insensitive) — YTM virtual list can render the same
        // track at multiple DOM positions, both consecutively and non-consecutively
        const seen = new Set();
        const all = raw.filter(item => {
            const key = item.title.toLowerCase();
            if (seen.has(key)) return false;
            seen.add(key);
            return true;
        });
        // During transitions YTM may mark multiple items as selected — use only the first
        const ci = all.findIndex(i => i.current);
        if (ci === -1) return [];
        all.forEach((item, idx) => { item.current = idx === ci; });
        if (pollCount % 10 === 1) {
            console.log('[ytune] queue raw=' + raw.length + ' deduped=' + all.length + ' currentIdx=' + ci);
            console.log('[ytune] queue items:', all.slice(Math.max(0,ci-1), ci+4).map(x => '"' + x.title + '" / "' + x.artist + '"' + (x.current?' [CUR]':'') + ' thumb=' + (x.thumb || 'NONE')));
        }
        return all.slice(Math.max(0, ci - 6), Math.min(all.length, ci + 7));
    }

    // Parse "m:ss", "mm:ss", or "h:mm:ss" text to seconds
    function parseMmSs(text) {
        if (!text) return 0;
        const parts = String(text).trim().split(':').map(Number);
        if (parts.some(isNaN)) return 0;
        if (parts.length === 3) return parts[0] * 3600 + parts[1] * 60 + parts[2];
        if (parts.length === 2) return parts[0] * 60 + parts[1];
        return 0;
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

    let _cachedVideoId = '';
    let _cachedVidTitle = '';
    function getVideoId() {
        const curTitle = document.querySelector('.title.ytmusic-player-bar')?.textContent?.trim() || '';
        if (_cachedVideoId && curTitle === _cachedVidTitle) return _cachedVideoId;
        _cachedVidTitle = curTitle;
        _cachedVideoId = '';
        // Best source: ytmusic-player's internal playerApi (works everywhere, instant)
        try {
            const p = document.querySelector('ytmusic-player');
            if (p?.playerApi?.getVideoData) {
                const d = p.playerApi.getVideoData();
                if (d?.video_id) { _cachedVideoId = d.video_id; return _cachedVideoId; }
            }
            if (p?.playerApi?.getPlayerResponse) {
                const r = p.playerApi.getPlayerResponse();
                if (r?.videoDetails?.videoId) { _cachedVideoId = r.videoDetails.videoId; return _cachedVideoId; }
            }
        } catch(e) {}
        const el = document.querySelector('ytmusic-player');
        if (el) {
            const id = el.getAttribute('video-id') || el.getAttribute('videoId');
            if (id) { _cachedVideoId = id; return id; }
        }
        const href = document.querySelector('ytmusic-player-queue-item[selected] a')?.href;
        if (href) {
            try { const v = new URL(href).searchParams.get('v') || ''; if (v) { _cachedVideoId = v; return v; } } catch(e) {}
        }
        try { const canon = document.querySelector('link[rel="canonical"]')?.href; if (canon) { const v = new URL(canon).searchParams.get('v') || ''; if (v) { _cachedVideoId = v; return v; } } } catch(e) {}
        // Fallback: extract video ID from the player bar thumbnail
        try {
            const thumb = document.querySelector('ytmusic-player-bar ytmusic-thumbnail img')?.src
                      || document.querySelector('ytmusic-player-bar img[src*="i.ytimg.com"]')?.src;
            if (thumb) {
                const m = thumb.match(/\/vi(?:_webp)?\/([a-zA-Z0-9_-]{11})\//);
                if (m) { _cachedVideoId = m[1]; return m[1]; }
            }
        } catch(e) {}
        // Fallback: queue item thumbnail
        try {
            const qiImg = document.querySelector('ytmusic-player-queue-item[selected] ytmusic-thumbnail img')?.src
                       || document.querySelector('ytmusic-player-queue-item[selected] img[src*="i.ytimg.com"]')?.src;
            if (qiImg) {
                const m = qiImg.match(/\/vi(?:_webp)?\/([a-zA-Z0-9_-]{11})\//);
                if (m) { _cachedVideoId = m[1]; return m[1]; }
            }
        } catch(e) {}
        // Fallback: check for video-id attributes on the selected queue item directly
        try {
            const qi = document.querySelector('ytmusic-player-queue-item[selected]');
            if (qi) {
                const attrs = ['video-id', 'videoId', 'data-video-id', 'data-id'];
                for (const a of attrs) {
                    const v = qi.getAttribute(a);
                    if (v) { _cachedVideoId = v; return v; }
                }
                const playBtn = qi.querySelector('ytmusic-item-thumbnail-overlay-play-button');
                if (playBtn) {
                    for (const a of attrs) {
                        const v = playBtn.getAttribute(a);
                        if (v) { _cachedVideoId = v; return v; }
                    }
                }
            }
        } catch(e) {}
        // Fallback: Media Session API artwork
        try {
            const ms = navigator.mediaSession?.metadata;
            if (ms?.artwork?.length) {
                const src = ms.artwork[0].src;
                if (src) {
                    const m = src.match(/\/vi(?:_webp)?\/([a-zA-Z0-9_-]{11})\//);
                    if (m) { _cachedVideoId = m[1]; return m[1]; }
                }
            }
        } catch(e) {}
        try {
            const qv = new URLSearchParams(window.location.search).get('v');
            if (qv) { _cachedVideoId = qv; return qv; }
        } catch(e) {}
        _cachedVidTitle = '';
        return '';
    }

    function cleanThumbUrl(url) {
        if (!url) return url;
        const qi = url.indexOf('?');
        return qi > 0 ? url.substring(0, qi) : url;
    }

    function upgradeThumbUrl(url) {
        if (!url) return url;
        // Google Photos CDN: resize to 640px square (path param, no query string)
        const googleMatch = url.match(/^(.*=)w\d+-h\d+/);
        if (googleMatch) return googleMatch[1] + 'w640-h640';
        // ytimg: don't upgrade quality — maxresdefault doesn't exist for all videos
        return url;
    }

    function getThumbUrl() {
        // Most reliable: Media Session API (set by YTM for all content types)
        try {
            const artwork = navigator.mediaSession.metadata?.artwork;
            if (artwork && artwork.length > 0) {
                const src = artwork[artwork.length - 1]?.src || artwork[0]?.src;
                if (src) {
                    // ytimg signed params (sqp/rs) require YouTube cookies — strip them
                    // so the base hqdefault.jpg URL loads in any context (popup, Discord etc)
                    // Google CDN URLs use path-based sizing — clean and upgrade those
                    if (src.includes('i.ytimg.com')) return cleanThumbUrl(src);
                    return upgradeThumbUrl(cleanThumbUrl(src));
                }
            }
        } catch(e) {}
        // Fallback: DOM selectors for album art
        const found = document.querySelector('ytmusic-player-bar ytmusic-thumbnail img')?.src
                   || document.querySelector('ytmusic-player-bar img[src*="googleusercontent"]')?.src;
        if (found) return upgradeThumbUrl(cleanThumbUrl(found));
        // Last resort: video ID — hqdefault always exists, maxresdefault does not
        const vid = getVideoId();
        if (vid) return 'https://i.ytimg.com/vi/' + vid + '/hqdefault.jpg';
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
            thumbnail:   thumb || _thumbData.url || '',
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
            shuffled:    document.querySelector('ytmusic-player-bar')?.hasAttribute('shuffle-on') ?? false,
            repeatMode: (function() {
                const mode = document.querySelector('ytmusic-player-bar')?.getAttribute('repeat-mode') || 'NONE';
                if (mode === 'ONE') return 'one';
                if (mode === 'ALL') return 'all';
                return 'none';
            })(),
            queue:       getQueue(),
            paletteH:      _palette.h,
            paletteS:      _palette.s,
            thumbnailData: _palette.url === thumb ? (_thumbData.display || _thumbData.data) : '',
            // Discord's image proxy can't fetch yt3.googleusercontent.com — use ytimg CDN instead.
            // Prefer thumb when it's already i.ytimg.com (Media Session API, updates immediately);
            // getVideoId() reads ytmusic-player[video-id] which can lag several polls behind.
            thumbnailDiscord: (function() {
                if (thumb && thumb.includes('i.ytimg.com')) return thumb;
                const vid = getVideoId();
                if (vid) return 'https://i.ytimg.com/vi/' + vid + '/maxresdefault.jpg';
                return thumb || '';
            })(),
            trackUrl: (function() {
                if (location.href.includes('music.youtube.com/watch')) return location.href;
                const vid = getVideoId();
                if (vid) return 'https://music.youtube.com/watch?v=' + vid;
                // Try from thumb URL directly (already computed above)
                if (thumb) {
                    const m = thumb.match(/\/vi(?:_webp)?\/([a-zA-Z0-9_-]{11})(?:\/|$)/);
                    if (m) return 'https://music.youtube.com/watch?v=' + m[1];
                }
                return '';
            })(),
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
        let lastVizTime = 0;
        const VIZ_INTERVAL = 33; // ~30 fps
        function loop(now) {
            if (!_vizRunning) return;
            if (now - lastVizTime >= VIZ_INTERVAL) {
                lastVizTime = now;
                if (_analyser && _audioCtx) {
                    if (_audioCtx.state === 'suspended') _audioCtx.resume().catch(() => {});
                    _analyser.getByteFrequencyData(_freqData);
                    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
                        event: 'ytune-viz',
                        payload: Array.from(_freqData),
                    }).catch(() => {});
                }
            }
            requestAnimationFrame(loop);
        }
        requestAnimationFrame(loop);
    }

    let pollCount = 0;
    let _lastStateKey = '';
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
            if (!state.title) return;
            window.__ytune__?.setVinylPlaying?.(state.playing);
            // Skip emit if nothing meaningful changed
            const stateKey = state.title + '|' + state.artist + '|' + state.playing + '|' + Math.floor(state.currentTime / 2) + '|' + state.trackUrl;
            if (stateKey === _lastStateKey && pollCount > 3) return;
            _lastStateKey = stateKey;
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
                color: hsl(${h2},${s}%,68%);
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

        // Notification bell + dedicated notification panel
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

        // Notification panel — separate from the menu modal
        const notifPanel = document.createElement('div');
        notifPanel.id = 'ytune-notif-panel';
        notifPanel.style.cssText = [
            'position:fixed', 'z-index:9999',
            'background:#1e1e1e', 'border:1px solid rgba(255,255,255,0.1)',
            'border-radius:14px', 'padding:16px', 'width:260px',
            'box-shadow:0 8px 32px rgba(0,0,0,0.6)',
            'display:none', 'flex-direction:column', 'gap:10px',
            'font-family:Roboto,sans-serif', 'color:#fff',
        ].join(';');

        const notifTitle = document.createElement('div');
        notifTitle.style.cssText = 'font-size:14px;font-weight:600;padding-bottom:8px;border-bottom:1px solid rgba(255,255,255,0.08);letter-spacing:0.3px';
        notifTitle.textContent = 'Notificações';

        const notifEmpty = document.createElement('div');
        notifEmpty.id = 'ytune-notif-empty';
        notifEmpty.style.cssText = 'font-size:12px;color:rgba(255,255,255,0.35);text-align:center;padding:12px 0';
        notifEmpty.textContent = 'Nenhuma notificação';

        notifPanel.appendChild(notifTitle);
        notifPanel.appendChild(notifEmpty);
        document.body.appendChild(notifPanel);

        bellBtn.addEventListener('click', (e) => {
            e.stopPropagation();
            modal.style.display = 'none';
            const visible = notifPanel.style.display === 'flex';
            notifPanel.style.display = visible ? 'none' : 'flex';
            if (!visible) {
                const r = bellBtn.getBoundingClientRect();
                notifPanel.style.top   = (r.bottom + 8) + 'px';
                notifPanel.style.right = (window.innerWidth - r.right) + 'px';
                // Clear badge when panel is opened
                badge.style.display = 'none';
            }
        });
        document.addEventListener('click', (e) => {
            if (!notifPanel.contains(e.target) && e.target !== bellBtn)
                notifPanel.style.display = 'none';
        });

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
                    // Restore token to Rust AuthTokenState (lost on app restart)
                    window.__TAURI_INTERNALS__?.invoke('plugin:event|emit', {
                        event: 'ytune-auth-restore', payload: tok
                    });
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
        dg.setAttribute("fill","none");
        dg.setAttribute("stroke","currentColor");
        function circ(cx,cy,r,sw,op) {
            const el=document.createElementNS(NS,"circle");
            el.setAttribute("cx",cx);el.setAttribute("cy",cy);
            el.setAttribute("r",r);el.setAttribute("stroke-width",sw);
            if(op!==undefined)el.setAttribute("opacity",op);
            dg.appendChild(el);
        }
        circ(512,512,460,30);
        circ(512,512,358,15,0.4);
        circ(512,512,256,15,0.4);
        circ(512,512,153,20);
        const tri=document.createElementNS(NS,"polygon");
        tri.setAttribute("points","491,450 491,573 594,512");
        tri.setAttribute("fill","currentColor");
        tri.setAttribute("stroke","none");
        dg.appendChild(tri);
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

    window.__ytune__.setNotifCount = function(n) {
        const badge = document.getElementById('ytune-notif-badge');
        if (!badge) return;
        badge.style.display = n > 0 ? 'block' : 'none';
    };

    window.__ytune__.showUpdateNotif = function(version) {
        const panel = document.getElementById('ytune-notif-panel');
        if (!panel) return;
        // Avoid duplicate cards
        if (document.getElementById('ytune-notif-update')) {
            document.getElementById('ytune-notif-update-ver').textContent = version;
            window.__ytune__.setNotifCount(1);
            return;
        }
        const empty = document.getElementById('ytune-notif-empty');
        if (empty) empty.style.display = 'none';

        const card = document.createElement('div');
        card.id = 'ytune-notif-update';
        card.style.cssText = 'background:rgba(48,209,88,0.08);border:1px solid rgba(48,209,88,0.2);border-radius:10px;padding:12px;display:flex;flex-direction:column;gap:10px';

        const cardTitle = document.createElement('div');
        cardTitle.style.cssText = 'font-size:13px;font-weight:600;color:#30d158';
        cardTitle.textContent = 'Atualização disponível';

        const cardVer = document.createElement('div');
        cardVer.id = 'ytune-notif-update-ver';
        cardVer.style.cssText = 'font-size:11px;color:rgba(255,255,255,0.5)';
        cardVer.textContent = version;

        const cardBtns = document.createElement('div');
        cardBtns.style.cssText = 'display:flex;gap:8px';

        const btnInstall = document.createElement('button');
        btnInstall.textContent = 'Instalar';
        btnInstall.style.cssText = 'flex:1;padding:6px 0;border-radius:8px;background:#30d158;color:#000;font-size:12px;font-weight:700;cursor:pointer;transition:opacity 0.15s';
        btnInstall.addEventListener('mouseenter', () => btnInstall.style.opacity = '0.85');
        btnInstall.addEventListener('mouseleave', () => btnInstall.style.opacity = '1');
        btnInstall.addEventListener('click', () => {
            btnInstall.textContent = 'Instalando…';
            btnInstall.style.opacity = '0.5';
            btnInstall.style.pointerEvents = 'none';
            window.__TAURI_INTERNALS__?.invoke('plugin:event|emit', {
                event: 'ytune-install-update', payload: {},
            }).catch(() => {});
        });

        const btnDismiss = document.createElement('button');
        btnDismiss.textContent = 'Dispensar';
        btnDismiss.style.cssText = 'flex:1;padding:6px 0;border-radius:8px;background:rgba(255,255,255,0.08);color:rgba(255,255,255,0.6);font-size:12px;cursor:pointer;transition:background 0.15s';
        btnDismiss.addEventListener('mouseenter', () => btnDismiss.style.background = 'rgba(255,255,255,0.14)');
        btnDismiss.addEventListener('mouseleave', () => btnDismiss.style.background = 'rgba(255,255,255,0.08)');
        btnDismiss.addEventListener('click', () => {
            card.remove();
            if (!panel.querySelector('div[id^="ytune-notif-"]:not(#ytune-notif-empty)'))
                if (empty) empty.style.display = 'block';
            window.__ytune__.setNotifCount(0);
        });

        cardBtns.appendChild(btnInstall);
        cardBtns.appendChild(btnDismiss);
        card.appendChild(cardTitle);
        card.appendChild(cardVer);
        card.appendChild(cardBtns);
        panel.appendChild(card);
        window.__ytune__.setNotifCount(1);
    };

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

    window.__ytune__.setRoomStatus = function(data) {
        let bar = document.getElementById('ytune-room-bar');
        if (!data || !data.roomId) {
            if (bar) bar.remove();
            return;
        }
        if (!bar) {
            bar = document.createElement('div');
            bar.id = 'ytune-room-bar';
            bar.style.cssText = [
                'position:fixed','bottom:76px','left:50%','transform:translateX(-50%)',
                'background:rgba(0,0,0,0.75)','backdrop-filter:blur(8px)',
                'border:1px solid rgba(255,255,255,0.12)','border-radius:20px',
                'padding:5px 14px','display:flex','align-items:center','gap:8px',
                'font-size:12px','color:#fff','z-index:9999','pointer-events:auto',
                'font-family:sans-serif','cursor:default',
            ].join(';');
            document.body.appendChild(bar);
        }
        const icon    = data.role === 'host' ? '\u{1F4E1}' : '\u{1F3B5}';
        const label   = data.role === 'host' ? 'Anfi\u{00E3}o' : 'Ouvindo junto';
        const members = data.memberCount > 0 ? ' \u{00B7} ' + data.memberCount + ' ouvindo' : '';
        bar.innerHTML = icon + ' <strong>' + data.roomId + '</strong>'
            + ' <span style="opacity:.6">' + label + members + '</span>';
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
