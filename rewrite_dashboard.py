import re

with open("cli/src/dashboard.rs", "r") as f:
    content = f.read()

new_html = r'''<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
    <title>PDOS Hub</title>
    <link href="https://fonts.googleapis.com/css2?family=SF+Pro+Display:wght@300;400;500;600&family=Inter:wght@400;500;600&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg-color: #000000;
            --glass-bg: rgba(30, 30, 30, 0.6);
            --glass-border: rgba(255, 255, 255, 0.1);
            --text-main: #ffffff;
            --text-muted: #98989d;
            --accent: #0a84ff;
            --success: #30d158;
            --danger: #ff453a;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
            -webkit-font-smoothing: antialiased;
        }

        body {
            font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", "Inter", sans-serif;
            background-color: var(--bg-color);
            background-image: 
                radial-gradient(circle at 15% 50%, rgba(10, 132, 255, 0.15), transparent 40%),
                radial-gradient(circle at 85% 30%, rgba(94, 92, 230, 0.15), transparent 40%);
            color: var(--text-main);
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            overflow: hidden;
        }

        /* Top Navigation */
        nav {
            width: 100%;
            padding: 20px 40px;
            display: flex;
            justify-content: space-between;
            align-items: center;
            z-index: 10;
        }

        .brand {
            font-size: 1.2rem;
            font-weight: 600;
            letter-spacing: -0.5px;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .version {
            font-size: 0.8rem;
            color: var(--text-muted);
            background: rgba(255,255,255,0.1);
            padding: 4px 10px;
            border-radius: 20px;
        }

        /* Main Container */
        .container {
            width: 100%;
            max-width: 800px;
            margin-top: 60px;
            padding: 0 20px;
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 40px;
        }

        h1 {
            font-weight: 500;
            font-size: 2.5rem;
            letter-spacing: -1px;
            text-align: center;
        }

        .subtitle {
            color: var(--text-muted);
            font-size: 1.1rem;
            text-align: center;
            margin-top: -30px;
        }

        /* Device Grid */
        .device-grid {
            display: flex;
            flex-wrap: wrap;
            justify-content: center;
            gap: 30px;
            width: 100%;
            min-height: 200px;
        }

        .device-orb {
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 12px;
            cursor: pointer;
            transition: all 0.3s cubic-bezier(0.25, 0.8, 0.25, 1);
            animation: fadeIn 0.5s ease-out forwards;
        }

        .device-orb:hover .orb-circle {
            transform: scale(1.05);
            box-shadow: 0 0 30px rgba(10, 132, 255, 0.3);
            border-color: rgba(255,255,255,0.3);
        }

        .device-orb:active .orb-circle {
            transform: scale(0.95);
        }

        .orb-circle {
            width: 80px;
            height: 80px;
            border-radius: 50%;
            background: var(--glass-bg);
            backdrop-filter: blur(20px);
            -webkit-backdrop-filter: blur(20px);
            border: 1px solid var(--glass-border);
            display: flex;
            justify-content: center;
            align-items: center;
            font-size: 32px;
            transition: all 0.3s ease;
            position: relative;
        }
        
        .orb-circle.mac { color: #ffffff; }
        .orb-circle.android { color: #3ddc84; }

        .device-name {
            font-size: 0.9rem;
            font-weight: 500;
        }
        
        .device-status {
            font-size: 0.75rem;
            color: var(--text-muted);
        }

        /* Scan Button */
        .scan-btn {
            background: rgba(255,255,255,0.1);
            border: 1px solid rgba(255,255,255,0.1);
            color: white;
            padding: 12px 24px;
            border-radius: 30px;
            font-size: 1rem;
            font-weight: 500;
            cursor: pointer;
            backdrop-filter: blur(10px);
            transition: all 0.2s ease;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .scan-btn:hover {
            background: rgba(255,255,255,0.15);
            transform: translateY(-2px);
        }

        /* Modal / Action Sheet */
        .modal-overlay {
            position: fixed;
            top: 0; left: 0; right: 0; bottom: 0;
            background: rgba(0,0,0,0.4);
            backdrop-filter: blur(10px);
            -webkit-backdrop-filter: blur(10px);
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
            background: rgba(30, 30, 30, 0.75);
            backdrop-filter: blur(30px);
            -webkit-backdrop-filter: blur(30px);
            border: 1px solid rgba(255, 255, 255, 0.15);
            border-radius: 24px;
            width: 100%;
            max-width: 500px;
            padding: 30px;
            transform: translateY(50px) scale(0.95);
            transition: all 0.4s cubic-bezier(0.16, 1, 0.3, 1);
            box-shadow: 0 20px 50px rgba(0,0,0,0.5);
            display: flex;
            flex-direction: column;
            gap: 24px;
        }

        .modal-overlay.active .action-sheet {
            transform: translateY(0) scale(1);
        }

        .sheet-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .sheet-header h2 {
            font-weight: 500;
            font-size: 1.5rem;
        }

        .close-btn {
            background: rgba(255,255,255,0.1);
            border: none;
            color: white;
            width: 32px; height: 32px;
            border-radius: 50%;
            display: flex;
            justify-content: center;
            align-items: center;
            cursor: pointer;
            transition: background 0.2s;
        }
        .close-btn:hover { background: rgba(255,255,255,0.2); }

        /* Action Grid */
        .action-grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 16px;
        }

        .action-card {
            background: rgba(255,255,255,0.05);
            border: 1px solid rgba(255,255,255,0.05);
            border-radius: 16px;
            padding: 20px;
            cursor: pointer;
            transition: all 0.2s ease;
            display: flex;
            flex-direction: column;
            align-items: flex-start;
            gap: 12px;
        }

        .action-card:hover {
            background: rgba(255,255,255,0.1);
            border-color: rgba(255,255,255,0.2);
        }

        .action-icon {
            font-size: 24px;
        }

        .action-title {
            font-weight: 500;
            font-size: 1rem;
        }

        /* Form Layouts within Modal */
        .sub-view {
            display: none;
            flex-direction: column;
            gap: 16px;
            animation: slideIn 0.3s forwards;
        }
        
        .sub-view.active { display: flex; }
        .main-view.hidden { display: none; }

        input, textarea {
            background: rgba(0,0,0,0.5);
            border: 1px solid rgba(255,255,255,0.1);
            border-radius: 12px;
            padding: 14px;
            color: white;
            font-family: inherit;
            font-size: 1rem;
            width: 100%;
        }

        input:focus, textarea:focus {
            outline: none;
            border-color: var(--accent);
        }

        .btn-primary {
            background: var(--accent);
            color: white;
            border: none;
            border-radius: 12px;
            padding: 14px;
            font-size: 1rem;
            font-weight: 600;
            cursor: pointer;
            width: 100%;
            transition: transform 0.1s, opacity 0.2s;
        }
        .btn-primary:active { transform: scale(0.98); }
        .btn-primary:hover { opacity: 0.9; }

        .back-btn {
            background: none;
            border: none;
            color: var(--accent);
            font-size: 1rem;
            display: flex;
            align-items: center;
            gap: 4px;
            cursor: pointer;
            margin-bottom: -10px;
            padding: 0;
            width: max-content;
        }

        /* Toast Notification */
        .toast-container {
            position: fixed;
            bottom: 40px;
            left: 50%;
            transform: translateX(-50%);
            display: flex;
            flex-direction: column;
            gap: 10px;
            z-index: 1000;
        }

        .toast {
            background: rgba(30, 30, 30, 0.85);
            backdrop-filter: blur(20px);
            -webkit-backdrop-filter: blur(20px);
            border: 1px solid rgba(255,255,255,0.1);
            padding: 12px 24px;
            border-radius: 30px;
            font-size: 0.95rem;
            font-weight: 500;
            display: flex;
            align-items: center;
            gap: 10px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.3);
            animation: toastEnter 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.275) forwards;
        }

        .toast.success { border-left: 4px solid var(--success); }
        .toast.error { border-left: 4px solid var(--danger); }

        @keyframes fadeIn { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: translateY(0); } }
        @keyframes slideIn { from { opacity: 0; transform: translateX(20px); } to { opacity: 1; transform: translateX(0); } }
        @keyframes toastEnter { from { opacity: 0; transform: translateY(20px); } to { opacity: 1; transform: translateY(0); } }
        @keyframes toastExit { from { opacity: 1; transform: translateY(0); } to { opacity: 0; transform: translateY(-20px); } }

        .terminal-output {
            background: black;
            font-family: monospace;
            padding: 12px;
            border-radius: 8px;
            font-size: 0.85rem;
            height: 150px;
            overflow-y: auto;
            color: #0f0;
            white-space: pre-wrap;
        }
    </style>
</head>
<body>

    <nav>
        <div class="brand">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/></svg>
            PDOS Hub
        </div>
        <div class="version">Beta</div>
    </nav>

    <div class="container">
        <div>
            <h1>Nearby Devices</h1>
            <p class="subtitle">Select a device to interact.</p>
        </div>

        <div class="device-grid" id="deviceGrid">
            <!-- Populated via JS -->
        </div>

        <button class="scan-btn" onclick="scanDevices()" id="btnScan">
            <svg width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"></path></svg>
            Scan for Devices
        </button>
    </div>

    <!-- Action Sheet Modal -->
    <div class="modal-overlay" id="actionModal" onclick="closeModal(event)">
        <div class="action-sheet" onclick="event.stopPropagation()">
            
            <!-- Main Grid View -->
            <div id="viewMain" class="main-view">
                <div class="sheet-header" style="margin-bottom: 24px;">
                    <div>
                        <h2 id="modalDeviceName">Device Name</h2>
                        <span id="modalDeviceId" style="font-size: 0.8rem; color: var(--text-muted);">ID</span>
                    </div>
                    <button class="close-btn" onclick="closeModal(true)"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg></button>
                </div>

                <div class="action-grid">
                    <div class="action-card" onclick="openSubView('viewNotify')">
                        <div class="action-icon">🔔</div>
                        <div class="action-title">Notification</div>
                    </div>
                    <div class="action-card" onclick="openSubView('viewClipboard')">
                        <div class="action-icon">📋</div>
                        <div class="action-title">Clipboard</div>
                    </div>
                    <div class="action-card" onclick="openSubView('viewTerminal')">
                        <div class="action-icon">💻</div>
                        <div class="action-title">Terminal</div>
                    </div>
                    <div class="action-card" onclick="openSubView('viewFile')">
                        <div class="action-icon">📁</div>
                        <div class="action-title">File Transfer</div>
                    </div>
                </div>
            </div>

            <!-- Sub View: Notification -->
            <div id="viewNotify" class="sub-view">
                <button class="back-btn" onclick="backToMain()">← Back</button>
                <h3>Send Notification</h3>
                <input type="text" id="notifyTitle" placeholder="Title">
                <input type="text" id="notifyBody" placeholder="Message">
                <button class="btn-primary" onclick="sendNotification()">Send</button>
            </div>

            <!-- Sub View: Clipboard -->
            <div id="viewClipboard" class="sub-view">
                <button class="back-btn" onclick="backToMain()">← Back</button>
                <h3>Sync Clipboard</h3>
                <textarea id="clipText" rows="3" placeholder="Paste text here to send..."></textarea>
                <div style="display: flex; gap: 10px;">
                    <button class="btn-primary" onclick="getClipboard()" style="background: rgba(255,255,255,0.1);">Fetch & Copy to Mac</button>
                    <button class="btn-primary" onclick="setClipboard(false)">Send Text</button>
                </div>
                <button class="btn-primary" onclick="setClipboard(true)" style="background: #8b5cf6;">Sync Mac Clipboard to Device</button>
            </div>

            <!-- Sub View: Terminal -->
            <div id="viewTerminal" class="sub-view">
                <button class="back-btn" onclick="backToMain()">← Back</button>
                <h3>Remote Terminal</h3>
                <div class="terminal-output" id="termOutput"></div>
                <div style="display: flex; gap: 10px;">
                    <input type="text" id="termCommand" placeholder="Command..." onkeydown="if(event.key === 'Enter') runCommand()">
                    <button class="btn-primary" style="width: auto; padding: 0 20px;" onclick="runCommand()">Run</button>
                </div>
            </div>

            <!-- Sub View: File Transfer -->
            <div id="viewFile" class="sub-view">
                <button class="back-btn" onclick="backToMain()">← Back</button>
                <h3>File Transfer</h3>
                <input type="text" id="fileLocal" placeholder="Local Path (Mac)">
                <input type="text" id="fileRemote" placeholder="Remote Path (Device)">
                <div style="display: flex; gap: 10px;">
                    <button class="btn-primary" onclick="sendFile()">Send to Device</button>
                    <button class="btn-primary" onclick="getFile()" style="background: rgba(255,255,255,0.1);">Fetch to Mac</button>
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
            toast.innerHTML = type === 'success' ? `✅ ${msg}` : `⚠️ ${msg}`;
            container.appendChild(toast);
            
            setTimeout(() => {
                toast.style.animation = 'toastExit 0.3s forwards';
                setTimeout(() => toast.remove(), 300);
            }, 3000);
        }

        async function scanDevices() {
            const btn = document.getElementById('btnScan');
            btn.innerHTML = `<span style="opacity:0.7">Scanning...</span>`;
            
            try {
                const response = await fetch('/api/devices');
                if (!response.ok) throw new Error("Connection failed");
                const devices = await response.json();
                
                const grid = document.getElementById('deviceGrid');
                grid.innerHTML = "";

                if (devices.length === 0) {
                    grid.innerHTML = `<div style="color:var(--text-muted); padding:40px;">No devices found.</div>`;
                } else {
                    devices.forEach((d, index) => {
                        const platformStr = typeof d.platform === 'object' ? 'unknown' : d.platform.toLowerCase();
                        const icon = platformStr === 'android' ? '📱' : (platformStr === 'mac' ? '💻' : '🖥️');
                        
                        const orb = document.createElement('div');
                        orb.className = 'device-orb';
                        orb.style.animationDelay = `${index * 0.1}s`;
                        orb.onclick = () => openModal(d.node_id, d.name);
                        
                        orb.innerHTML = `
                            <div class="orb-circle ${platformStr}">${icon}</div>
                            <div class="device-name">${d.name}</div>
                            <div class="device-status">Connected</div>
                        `;
                        grid.appendChild(orb);
                    });
                    if (devices.length > 0) showToast(`Found ${devices.length} devices`);
                }
            } catch (err) {
                showToast(err.message, 'error');
            } finally {
                btn.innerHTML = `<svg width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"></path></svg> Rescan`;
            }
        }

        function openModal(id, name) {
            selectedNodeId = id;
            document.getElementById('modalDeviceName').innerText = name;
            document.getElementById('modalDeviceId').innerText = id.split('-')[0];
            
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
                if (res.ok) showToast("Notification sent");
                else showToast("Failed to send", "error");
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
                    showToast("Copied to Mac clipboard!");
                }
                else showToast("Failed to get clipboard", "error");
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
                if (res.ok) showToast(useLocal ? "Mac clipboard synced!" : "Text sent to device!");
                else showToast("Failed to set clipboard", "error");
            } catch (err) { showToast(err.message, "error"); }
        }

        async function runCommand() {
            const cmd = document.getElementById('termCommand').value;
            const out = document.getElementById('termOutput');
            out.innerHTML += `\n$ ${cmd}`;
            try {
                const res = await fetch('/api/exec', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ node_id: selectedNodeId, command: cmd })
                });
                const data = await res.json();
                if (res.ok) out.innerHTML += `\n${data.output}`;
                else out.innerHTML += `\nError: ${data.error}`;
            } catch (err) { out.innerHTML += `\nError: ${err.message}`; }
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
                if (res.ok) showToast("File sent successfully!");
                else showToast("Failed to send file", "error");
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
                if (res.ok) showToast("File fetched successfully!");
                else showToast("Failed to fetch file", "error");
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

