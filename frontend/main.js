// VRCRecorder Frontend — uses Tauri v2 __TAURI_INTERNALS__ polling
(function() {
    var ipc = window.__TAURI_INTERNALS__;
    var $ = function(id) { return document.getElementById(id); };
    var status = function(msg) { var el = $('status-info'); if (el) el.textContent = msg; console.log('[status]', msg); };

    status('JS loaded');

    if (!ipc) {
        status('ERROR: __TAURI_INTERNALS__ missing');
        return;
    }

    var invoke = function(cmd, args) { return ipc.invoke(cmd, args || {}); };
    status('IPC OK');

    function playSound(path) {
        var audio = new Audio(path);
        audio.volume = 0.2;
        audio.play().catch(function() {});
    }

    var state = {
        config: null,
        senders: [],
        connected: false,
        senderName: null,
        recording: false,
        elapsedSecs: 0,
    };

    function setToggle(id, on) { $(id).classList.toggle('active', on); }
    function toggleEl(id) { $(id).classList.toggle('active'); return $(id).classList.contains('active'); }

    // ---- Event dispatch (from polled events) ----
    function handleEvent(evt) {
        var d = evt.data || evt;
        switch (evt.type) {
            case 'SenderListUpdated':
                state.senders = d.senders || [];
                var c = $('sender-list');
                if (!state.senders.length) {
                    c.innerHTML = '<div class="setting-item" style="color:rgba(255,255,255,0.3);font-style:italic;">No senders found</div>';
                } else {
                    c.innerHTML = state.senders.map(function(s) {
                        return '<div class="setting-item" onclick="connectSender(\'' + s + '\')"><div class="sender-dot"></div><span class="sender-name">' + s + '</span></div>';
                    }).join('');
                }
                break;

            case 'SenderConnected':
                state.connected = true; state.senderName = d.name;
                status('Connected: ' + d.name + ' ' + d.width + 'x' + d.height);
                $('meta-resolution').textContent = d.width + 'x' + d.height;
                break;

            case 'SenderDisconnected':
                state.connected = false; state.senderName = null;
                status('Not connected');
                $('no-signal').style.display = 'block';
                $('meta-resolution').textContent = '--';
                break;

            case 'RecordingStarted':
                state.recording = true;
                $('recordBtn').classList.add('recording');
                $('live-tag').style.color = '#FF3B30';
                $('live-tag-text').textContent = 'REC 00:00:00';
                playSound('sound/start.mp3');
                break;

            case 'RecordingStopped':
                state.recording = false; state.elapsedSecs = 0;
                $('recordBtn').classList.remove('recording');
                $('live-tag').style.color = '';
                $('live-tag-text').textContent = 'PREVIEW MODE';
                $('meta-elapsed').textContent = '00:00:00';
                playSound('sound/stop.mp3');
                break;

            case 'RecordingElapsed':
                state.elapsedSecs = d.elapsed_secs;
                var h = Math.floor(state.elapsedSecs / 3600);
                var m = Math.floor((state.elapsedSecs % 3600) / 60);
                var s = state.elapsedSecs % 60;
                var fmt = String(h).padStart(2,'0') + ':' + String(m).padStart(2,'0') + ':' + String(s).padStart(2,'0');
                $('meta-elapsed').textContent = fmt;
                if (state.recording) $('live-tag-text').textContent = 'REC ' + fmt;
                break;

            // PreviewFrame is handled via binary polling (see pollPreview below)

            case 'Error':
                status('Error: ' + (d.message || ''));
                break;

            case 'ConfigChanged':
                state.config.mirror_video = d.mirror_video;
                state.config.portrait_mode = d.portrait_mode;
                state.config.video_vflip = d.video_vflip;
                state.config.preview_enabled = d.preview_enabled;
                setToggle('portrait-toggle', d.portrait_mode);
                setToggle('dock-portrait', d.portrait_mode);
                setToggle('mirror-toggle', d.mirror_video);
                setToggle('dock-mirror', d.mirror_video);
                setToggle('vflip-toggle', d.video_vflip);
                setToggle('dock-vflip', d.video_vflip);
                setToggle('preview-toggle', d.preview_enabled);
                if (d.preview_enabled) startPreview(); else stopPreview();
                break;

            case 'ResolutionChanged':
                status('Resolution changed - restarting...');
                break;
        }
    }

    // ---- Polling loop ----
    function poll() {
        invoke('poll_events').then(function(events) {
            if (Array.isArray(events)) {
                for (var i = 0; i < events.length; i++) {
                    handleEvent(events[i]);
                }
            }
        }).catch(function() {});
    }

    setInterval(poll, 300);

    // ---- Preview via MJPEG stream (browser-native, no polling) ----
    function startPreview() {
        invoke('get_stream_port').then(function(port) {
            var url = 'http://127.0.0.1:' + port + '/stream';
            var img = $('preview-img');
            img.onload = function() {
                $('no-signal').style.display = 'none';
            };
            img.src = url;
            img.style.display = '';
            $('no-signal').textContent = 'No Signal';
        }).catch(function(e) {
            status('MJPEG port error: ' + e);
        });
    }

    function stopPreview() {
        var img = $('preview-img');
        img.src = '';
        img.style.display = 'none';
        $('no-signal').textContent = 'Preview Off';
        $('no-signal').style.display = 'block';
    }

    if (state.config && state.config.preview_enabled !== false) {
        startPreview();
    } else {
        stopPreview();
    }

    // ---- Tauri Commands ----
    window.connectSender = function(name) { invoke('connect_sender', { name: name }).catch(function(e) { status('Connect error: ' + e); }); };
    window.toggleRecord = function() {
        if (state.recording) invoke('stop_recording').catch(function(e) { status('Stop error: ' + e); });
        else invoke('start_recording').catch(function(e) { status('Start error: ' + e); });
    };

    // ---- UI Bindings ----
    $('osc-enable-row').addEventListener('click', function() {
        if (!state.config) return status('Config not loaded yet');
        state.config.osc_enabled = toggleEl('osc-toggle');
        $('osc-details').style.display = state.config.osc_enabled ? 'block' : 'none';
        invoke('update_config', { config: state.config }).catch(function(e) { status('Config save error: ' + e); });
    });

    $('auto-camera-row').addEventListener('click', function() {
        if (!state.config) return;
        state.config.auto_camera = toggleEl('auto-camera-toggle');
        invoke('update_config', { config: state.config }).catch(function() {});
    });

    $('mirror-row').addEventListener('click', function() {
        if (!state.config) return;
        state.config.mirror_video = toggleEl('mirror-toggle');
        setToggle('dock-mirror', state.config.mirror_video);
        invoke('update_config', { config: state.config }).catch(function() {});
    });

    $('portrait-row').addEventListener('click', function() {
        if (!state.config) return;
        state.config.portrait_mode = toggleEl('portrait-toggle');
        setToggle('dock-portrait', state.config.portrait_mode);
        invoke('update_config', { config: state.config }).catch(function() {});
    });

    $('preview-row').addEventListener('click', function() {
        if (!state.config) return;
        state.config.preview_enabled = toggleEl('preview-toggle');
        if (state.config.preview_enabled) startPreview(); else stopPreview();
        invoke('update_config', { config: state.config }).catch(function() {});
    });

    $('vflip-row').addEventListener('click', function() {
        if (!state.config) return;
        state.config.video_vflip = toggleEl('vflip-toggle');
        setToggle('dock-vflip', state.config.video_vflip);
        invoke('update_config', { config: state.config }).catch(function() {});
    });

    $('recordBtn').addEventListener('click', function() { window.toggleRecord(); });
    $('dock-mirror').addEventListener('click', function() { $('mirror-row').click(); });
    $('dock-portrait').addEventListener('click', function() { $('portrait-row').click(); });
    $('dock-vflip').addEventListener('click', function() { $('vflip-row').click(); });

    $('osc-port').addEventListener('change', function() {
        if (!state.config) return;
        state.config.osc_port = this.value;
        invoke('update_config', { config: state.config }).catch(function() {});
    });
    $('osc-address').addEventListener('change', function() {
        if (!state.config) return;
        state.config.osc_record_address = this.value;
        invoke('update_config', { config: state.config }).catch(function() {});
    });
    $('bitrate-select').addEventListener('change', function() {
        if (!state.config) return;
        state.config.video_bitrate = this.value;
        invoke('update_config', { config: state.config }).catch(function() {});
    });

    // ---- Init ----
    invoke('get_config').then(function(cfg) {
        state.config = cfg;
        status('Config loaded OK');
        setToggle('osc-toggle', cfg.osc_enabled);
        $('osc-details').style.display = cfg.osc_enabled ? 'block' : 'none';
        $('osc-port').value = cfg.osc_port || '9001';
        $('osc-address').value = cfg.osc_record_address || '/avatar/parameters/VRCRecord';
        setToggle('auto-camera-toggle', cfg.auto_camera !== false);
        $('bitrate-select').value = cfg.video_bitrate || '2M';
        setToggle('mirror-toggle', cfg.mirror_video);
        setToggle('dock-mirror', cfg.mirror_video);
        setToggle('portrait-toggle', cfg.portrait_mode);
        setToggle('dock-portrait', cfg.portrait_mode);
        setToggle('vflip-toggle', cfg.video_vflip);
        setToggle('dock-vflip', cfg.video_vflip);
        setToggle('preview-toggle', cfg.preview_enabled !== false);
        $('meta-bitrate').textContent = cfg.video_bitrate || '2M';

        // Start or skip preview based on saved config
        if (cfg.preview_enabled !== false) {
            startPreview();
        } else {
            stopPreview();
        }
    }).catch(function(e) {
        status('Config load FAILED: ' + (e ? e.toString() : 'unknown'));
    });

    status('Init complete');
})();
