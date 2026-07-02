import re

with open("cli/src/dashboard.rs", "r") as f:
    content = f.read()

new_html = r'''<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
    <title>PDOS / HUB</title>
    <link href="https://fonts.googleapis.com/css2?family=Space+Mono:ital,wght@0,400;0,700;1,400&family=Inter:wght@400;500;600&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg: #000000;
            --fg: #ffffff;
            --muted: #666666;
            --border: #333333;
            --hover-bg: #ffffff;
            --hover-fg: #000000;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
            -webkit-font-smoothing: antialiased;
        }

        body {
            font-family: "Space Mono", monospace;
            background-color: var(--bg);
            color: var(--fg);
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            overflow: hidden;
            text-transform: uppercase;
        }

        /* Top Navigation */
        nav {
            width: 100%;
            padding: 24px 40px;
            display: flex;
            justify-content: space-between;
            align-items: center;
            border-bottom: 1px solid var(--border);
            z-index: 10;
        }

        .brand {
            font-size: 1.2rem;
            font-weight: 700;
            letter-spacing: 2px;
            display: flex;
            align-items: center;
            gap: 12px;
        }

        .version {
            font-size: 0.8rem;
            color: var(--muted);
            border: 1px solid var(--border);
            padding: 4px 12px;
        }

        /* Main Container */
        .container {
            width: 100%;
            max-width: 900px;
            margin-top: 60px;
            padding: 0 20px;
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 60px;
        }

        h1 {
            font-weight: 400;
            font-size: 2rem;
            letter-spacing: 4px;
            text-align: center;
        }

        .subtitle {
            color: var(--muted);
            font-size: 0.9rem;
            text-align: center;
            margin-top: -40px;
            letter-spacing: 1px;
        }

        /* Device Grid */
        .device-grid {
            display: flex;
            flex-wrap: wrap;
            justify-content: center;
            gap: 40px;
            width: 100%;
            min-height: 200px;
        }

        .device-card {
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 16px;
            cursor: pointer;
            padding: 24px;
            border: 1px solid var(--border);
            transition: all 0.2s ease;
            width: 200px;
        }

        .device-card:hover {
            background: var(--hover-bg);
            color: var(--hover-fg);
            border-color: var(--fg);
        }

        .device-card:hover .device-status {
            color: var(--hover-fg);
        }

        .device-icon {
            font-size: 32px;
            font-family: "Inter", sans-serif;
        }

        .device-name {
            font-size: 0.85rem;
            font-weight: 700;
            text-align: center;
            word-break: break-all;
        }
        
        .device-status {
            font-size: 0.75rem;
            color: var(--muted);
            display: flex;
            align-items: center;
            gap: 6px;
            transition: color 0.2s;
        }
        
        .status-dot {
            width: 6px;
            height: 6px;
            background: currentColor;
            border-radius: 50%;
        }

        /* Scan Button */
        .scan-btn {
            background: var(--bg);
            border: 1px solid var(--fg);
            color: var(--fg);
            padding: 16px 32px;
            font-size: 0.9rem;
            font-family: inherit;
            text-transform: uppercase;
            letter-spacing: 2px;
            cursor: pointer;
            transition: all 0.2s ease;
        }

        .scan-btn:hover {
            background: var(--fg);
            color: var(--bg);
        }

        /* Modal / Action Sheet */
        .modal-overlay {
            position: fixed;
            top: 0; left: 0; right: 0; bottom: 0;
            background: rgba(0,0,0,0.9);
            display: flex;
            justify-content: center;
            align-items: center;
            opacity: 0;
            pointer-events: none;
            transition: opacity 0.3s ease;
            z-index: 100;
        }

        .modal-overlay.active {
            opacity: 1;
            pointer-events: auto;
        }

        .action-sheet {
            background: var(--bg);
            border: 1px solid var(--border);
            width: 100%;
            max-width: 600px;
            padding: 40px;
            display: flex;
            flex-direction: column;
            gap: 32px;
        }

        .sheet-header {
            display: flex;
            justify-content: space-between;
            align-items: flex-start;
            border-bottom: 1px solid var(--border);
            padding-bottom: 20px;
        }

        .sheet-header h2 {
            font-weight: 400;
            font-size: 1.2rem;
            letter-spacing: 2px;
        }

        .close-btn {
            background: none;
            border: none;
            color: var(--fg);
            font-family: inherit;
            font-size: 1rem;
            cursor: pointer;
            text-decoration: underline;
        }
        .close-btn:hover { color: var(--muted); }

        /* Action Grid */
        .action-grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 20px;
        }

        .action-btn {
            background: var(--bg);
            border: 1px solid var(--border);
            color: var(--fg);
            padding: 24px;
            cursor: pointer;
            transition: all 0.2s ease;
            display: flex;
            flex-direction: column;
            align-items: flex-start;
            gap: 16px;
            font-family: inherit;
            text-transform: uppercase;
        }

        .action-btn:hover {
            background: var(--fg);
            color: var(--bg);
            border-color: var(--fg);
        }

        .action-title {
            font-size: 0.9rem;
            letter-spacing: 1px;
        }

        /* Form Layouts within Modal */
        .sub-view {
            display: none;
            flex-direction: column;
            gap: 24px;
        }
        
        .sub-view.active { display: flex; }
        .main-view.hidden { display: none; }

        input, textarea {
            background: var(--bg);
            border: 1px solid var(--border);
            padding: 16px;
            color: var(--fg);
            font-family: inherit;
            font-size: 0.9rem;
            width: 100%;
            border-radius: 0;
        }

        input:focus, textarea:focus {
            outline: none;
            border-color: var(--fg);
        }

        .btn-primary {
            background: var(--fg);
            color: var(--bg);
            border: 1px solid var(--fg);
            padding: 16px;
            font-size: 0.9rem;
            font-family: inherit;
            text-transform: uppercase;
            letter-spacing: 2px;
            cursor: pointer;
            width: 100%;
            transition: all 0.2s;
        }
        .btn-primary:hover { 
            background: var(--bg);
            color: var(--fg);
        }

        .btn-secondary {
            background: var(--bg);
            color: var(--fg);
            border: 1px solid var(--border);
            padding: 16px;
            font-size: 0.9rem;
            font-family: inherit;
            text-transform: uppercase;
            letter-spacing: 2px;
            cursor: pointer;
            width: 100%;
            transition: all 0.2s;
        }
        .btn-secondary:hover {
            border-color: var(--fg);
        }

        .back-btn {
            background: none;
            border: none;
            color: var(--muted);
            font-family: inherit;
            font-size: 0.85rem;
            text-transform: uppercase;
            letter-spacing: 1px;
            cursor: pointer;
            text-align: left;
            margin-bottom: -10px;
        }
        .back-btn:hover { color: var(--fg); }

        /* Toast Notification */
        .toast-container {
            position: fixed;
            bottom: 40px;
            right: 40px;
            display: flex;
            flex-direction: column;
            gap: 16px;
            z-index: 1000;
        }

        .toast {
            background: var(--fg);
            color: var(--bg);
            border: 1px solid var(--fg);
            padding: 16px 24px;
            font-size: 0.85rem;
            letter-spacing: 1px;
            display: flex;
            align-items: center;
            gap: 12px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.5);
        }

        .toast.error { 
            background: var(--bg);
            color: var(--fg);
            border-color: var(--fg);
        }

        .terminal-output {
            background: var(--bg);
            border: 1px solid var(--border);
            font-family: inherit;
            padding: 16px;
            font-size: 0.85rem;
            height: 200px;
            overflow-y: auto;
            color: var(--fg);
            white-space: pre-wrap;
            text-transform: none;
        }
    </style>
</head>
<body>

    <nav>
        <div class="brand">
            [ PDOS ]
        </div>
        <div class="version">V0.3</div>
    </nav>

    <div class="container">
        <div>
            <h1>SYSTEM NODES</h1>
            <p class="subtitle">SELECT TARGET NODE</p>
        </div>

        <div class="device-grid" id="deviceGrid">
            <!-- Populated via JS -->
        </div>

        <button class="scan-btn" onclick="scanDevices()" id="btnScan">
            [ SCAN NETWORK ]
        </button>
    </div>

    <!-- Action Sheet Modal -->
    <div class="modal-overlay" id="actionModal" onclick="closeModal(event)">
        <div class="action-sheet" onclick="event.stopPropagation()">
            
            <!-- Main Grid View -->
            <div id="viewMain" class="main-view">
                <div class="sheet-header">
                    <div>
                        <h2 id="modalDeviceName">NODE_NAME</h2>
                        <span id="modalDeviceId" style="font-size: 0.75rem; color: var(--muted);">ID_STRING</span>
                    </div>
                    <button class="close-btn" onclick="closeModal(true)">[ CLOSE ]</button>
                </div>

                <div class="action-grid">
                    <button class="action-btn" onclick="openSubView('viewNotify')">
                        <span class="action-title">NOTIFICATION</span>
                    </button>
                    <button class="action-btn" onclick="openSubView('viewClipboard')">
                        <span class="action-title">CLIPBOARD</span>
                    </button>
                    <button class="action-btn" onclick="openSubView('viewTerminal')">
                        <span class="action-title">TERMINAL</span>
                    </button>
                    <button class="action-btn" onclick="openSubView('viewFile')">
                        <span class="action-title">FILE TRANSFER</span>
                    </button>
                </div>
            </div>

            <!-- Sub View: Notification -->
            <div id="viewNotify" class="sub-view">
                <button class="back-btn" onclick="backToMain()">← BACK</button>
                <div class="sheet-header"><h2>NOTIFICATION</h2></div>
                <input type="text" id="notifyTitle" placeholder="TITLE">
                <input type="text" id="notifyBody" placeholder="BODY">
                <button class="btn-primary" onclick="sendNotification()">DISPATCH</button>
            </div>

            <!-- Sub View: Clipboard -->
            <div id="viewClipboard" class="sub-view">
                <button class="back-btn" onclick="backToMain()">← BACK</button>
                <div class="sheet-header"><h2>CLIPBOARD</h2></div>
                <textarea id="clipText" rows="4" placeholder="CONTENT..." style="text-transform:none;"></textarea>
                <div style="display: flex; gap: 16px;">
                    <button class="btn-secondary" onclick="getClipboard()">FETCH TO MAC</button>
                    <button class="btn-secondary" onclick="setClipboard(false)">PUSH TEXT</button>
                </div>
                <button class="btn-primary" onclick="setClipboard(true)">SYNC MAC -> NODE</button>
            </div>

            <!-- Sub View: Terminal -->
            <div id="viewTerminal" class="sub-view">
                <button class="back-btn" onclick="backToMain()">← BACK</button>
                <div class="sheet-header"><h2>REMOTE TERMINAL</h2></div>
                <div class="terminal-output" id="termOutput"></div>
                <div style="display: flex; gap: 16px;">
                    <input type="text" id="termCommand" placeholder="COMMAND" style="text-transform:none;" onkeydown="if(event.key === 'Enter') runCommand()">
                    <button class="btn-primary" style="width: auto; padding: 0 32px;" onclick="runCommand()">EXECUTE</button>
                </div>
            </div>

            <!-- Sub View: File Transfer -->
            <div id="viewFile" class="sub-view">
                <button class="back-btn" onclick="backToMain()">← BACK</button>
                <div class="sheet-header"><h2>FILE TRANSFER</h2></div>
                <input type="text" id="fileLocal" placeholder="LOCAL PATH (MAC)" style="text-transform:none;">
                <input type="text" id="fileRemote" placeholder="REMOTE PATH (NODE)" style="text-transform:none;">
                <div style="display: flex; gap: 16px;">
                    <button class="btn-primary" onclick="sendFile()">PUSH TO NODE</button>
                    <button class="btn-secondary" onclick="getFile()">PULL FROM NODE</button>
                </div>
            </div>

        </div>
    </div>

    <div class="toast-container" id="toastContainer"></div>

    <script>
        let selectedNodeId = null;

        function showToast(msg, type = 'success') {
            const container = document.getElementById('toastContainer');
            const toast = document.createElement('div');
            toast.className = `toast ${type}`;
            toast.innerHTML = type === 'success' ? `[OK] ${msg}` : `[ERR] ${msg}`;
            container.appendChild(toast);
            
            setTimeout(() => {
                toast.style.opacity = '0';
                setTimeout(() => toast.remove(), 300);
            }, 3000);
        }

        async function scanDevices() {
            const btn = document.getElementById('btnScan');
            btn.innerHTML = `[ SCANNING... ]`;
            
            try {
                const response = await fetch('/api/devices');
                if (!response.ok) throw new Error("Connection failed");
                const devices = await response.json();
                
                const grid = document.getElementById('deviceGrid');
                grid.innerHTML = "";

                if (devices.length === 0) {
                    grid.innerHTML = `<div style="color:var(--muted); padding:40px;">[ NO NODES DETECTED ]</div>`;
                } else {
                    devices.forEach((d) => {
                        const platformStr = typeof d.platform === 'object' ? 'unknown' : d.platform.toLowerCase();
                        let icon = "PC";
                        if (platformStr === 'mac') icon = "MAC";
                        if (platformStr === 'android') icon = "AND";
                        
                        const card = document.createElement('div');
                        card.className = 'device-card';
                        card.onclick = () => openModal(d.node_id, d.name);
                        
                        card.innerHTML = `
                            <div class="device-icon">${icon}</div>
                            <div class="device-name">${d.name}</div>
                            <div class="device-status"><span class="status-dot"></span> ONLINE</div>
                        `;
                        grid.appendChild(card);
                    });
                    if (devices.length > 0) showToast(`FOUND ${devices.length} NODE(S)`);
                }
            } catch (err) {
                showToast(err.message, 'error');
            } finally {
                btn.innerHTML = `[ SCAN NETWORK ]`;
            }
        }

        function openModal(id, name) {
            selectedNodeId = id;
            document.getElementById('modalDeviceName').innerText = name.toUpperCase();
            document.getElementById('modalDeviceId').innerText = id.split('-')[0].toUpperCase();
            
            backToMain();
            document.getElementById('actionModal').classList.add('active');
        }

        function closeModal(event) {
            if (event === true || event.target.id === 'actionModal') {
                document.getElementById('actionModal').classList.remove('active');
                setTimeout(backToMain, 300);
            }
        }

        function openSubView(id) {
            document.getElementById('viewMain').classList.add('hidden');
            document.getElementById(id).classList.add('active');
        }

        function backToMain() {
            document.querySelectorAll('.sub-view').forEach(el => el.classList.remove('active'));
            document.getElementById('viewMain').classList.remove('hidden');
        }

        // Action Implementations
        async function sendNotification() {
            const title = document.getElementById('notifyTitle').value;
            const body = document.getElementById('notifyBody').value;
            try {
                const res = await fetch('/api/notify', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId, title, body })
                });
                if (res.ok) showToast("NOTIFICATION DISPATCHED");
                else showToast("DISPATCH FAILED", "error");
            } catch (err) { showToast(err.message, "error"); }
        }

        async function getClipboard() {
            try {
                const res = await fetch('/api/clipboard/get', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId })
                });
                const data = await res.json();
                if (res.ok) {
                    document.getElementById('clipText').value = data.content;
                    showToast("FETCHED TO LOCAL CLIPBOARD");
                }
                else showToast("FETCH FAILED", "error");
            } catch (err) { showToast(err.message, "error"); }
        }

        async function setClipboard(useLocal) {
            let content = useLocal ? "" : document.getElementById('clipText').value;
            try {
                const res = await fetch('/api/clipboard/set', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId, content, use_local: useLocal })
                });
                if (res.ok) showToast(useLocal ? "MAC CLIPBOARD SYNCED" : "TEXT PUSHED");
                else showToast("SYNC FAILED", "error");
            } catch (err) { showToast(err.message, "error"); }
        }

        async function runCommand() {
            const cmd = document.getElementById('termCommand').value;
            const out = document.getElementById('termOutput');
            out.innerHTML += `\n> ${cmd}`;
            try {
                const res = await fetch('/api/exec', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId, command: cmd })
                });
                const data = await res.json();
                if (res.ok) out.innerHTML += `\n${data.output}`;
                else out.innerHTML += `\n[ERR] ${data.error}`;
            } catch (err) { out.innerHTML += `\n[ERR] ${err.message}`; }
            out.scrollTop = out.scrollHeight;
            document.getElementById('termCommand').value = '';
        }

        async function sendFile() {
            const local = document.getElementById('fileLocal').value;
            const remote = document.getElementById('fileRemote').value;
            try {
                const res = await fetch('/api/send-file', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId, local_path: local, remote_path: remote })
                });
                if (res.ok) showToast("FILE TRANSFER COMPLETE");
                else showToast("TRANSFER FAILED", "error");
            } catch (err) { showToast(err.message, "error"); }
        }

        async function getFile() {
            const local = document.getElementById('fileLocal').value;
            const remote = document.getElementById('fileRemote').value;
            try {
                const res = await fetch('/api/get-file', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId, local_path: local, remote_path: remote })
                });
                if (res.ok) showToast("FILE FETCH COMPLETE");
                else showToast("FETCH FAILED", "error");
            } catch (err) { showToast(err.message, "error"); }
        }

        // Initial scan
        window.onload = () => setTimeout(scanDevices, 500);
    </script>
</body>
</html>'''

pattern = r'fn get_dashboard_html\(\) -> String \{[\s\S]*?\}\n'
replacement = f'fn get_dashboard_html() -> String {{\n    r#"{new_html}"#\n        .to_string()\n}}\n'

new_content = re.sub(pattern, replacement, content)

with open("cli/src/dashboard.rs", "w") as f:
    f.write(new_content)

