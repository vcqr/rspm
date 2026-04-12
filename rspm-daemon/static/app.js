const API_BASE = '/api';
let autoRefresh = true;
let refreshInterval = null;
let currentDetailProcess = null;
let currentLogsProcess = null;
let currentLogsTab = 'stdout'; // Track current active tab
let logsAutoRefresh = false;
let logsInterval = 5;
let logsTimer = null;
let deleteProcessId = null;
let deleteScheduleId = null;
let isDarkTheme = true;
let currentMainTab = localStorage.getItem('currentMainTab') || 'processes';

// Theme toggle
function toggleTheme() {
    isDarkTheme = !isDarkTheme;
    document.documentElement.classList.toggle('dark', isDarkTheme);
    document.getElementById('theme-icon-sun').style.display = isDarkTheme ? 'none' : 'block';
    document.getElementById('theme-icon-moon').style.display = isDarkTheme ? 'block' : 'none';
    localStorage.setItem('theme', isDarkTheme ? 'dark' : 'light');
}

// Initialize theme
function initTheme() {
    const savedTheme = localStorage.getItem('theme');
    if (savedTheme === 'light') {
        isDarkTheme = false;
        document.documentElement.classList.remove('dark');
        document.getElementById('theme-icon-sun').style.display = 'block';
        document.getElementById('theme-icon-moon').style.display = 'none';
    }
}

// Icon helper - Using SVG sprite (Lucide icons)
function icon(name, className = 'icon', strokeWidth = 2, color = null) {
    const style = color ? ` style="color: ${color};"` : '';
    return `<svg class="${className}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round"${style}><use href="#icon-${name}"/></svg>`;
}

async function fetchData(endpoint) {
    try {
        const response = await fetch(`${API_BASE}${endpoint}`);
        const data = await response.json();
        if (!response.ok) throw new Error(data.error || 'Request failed');
        return data;
    } catch (error) {
        showError(error.message);
        return null;
    }
}

async function postData(endpoint, data = {}) {
    try {
        const response = await fetch(`${API_BASE}${endpoint}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(data)
        });
        const result = await response.json();
        if (!response.ok) throw new Error(result.error || 'Request failed');
        return result;
    } catch (error) {
        showError(error.message);
        return null;
    }
}

function showError(message) {
    const alert = document.getElementById('error-alert');
    document.getElementById('error-message').textContent = message;
    alert.style.display = 'flex';
    setTimeout(() => { alert.style.display = 'none'; }, 5000);
}

// ============ Custom Confirm/Alert Dialogs ============
let confirmCallback = null;
let confirmMessage = '';

function showConfirm(message, onConfirm) {
    confirmMessage = message;
    confirmCallback = onConfirm;
    document.getElementById('confirm-message').textContent = message;
    document.getElementById('confirm-modal').classList.add('active');
}

function closeConfirmModal() {
    document.getElementById('confirm-modal').classList.remove('active');
    confirmCallback = null;
}

function confirmOk() {
    const callback = confirmCallback;
    closeConfirmModal();
    if (callback) callback();
}

function showAlert(message, title = 'Notice') {
    document.getElementById('alert-message').textContent = message;
    document.getElementById('alert-title').textContent = title;
    document.getElementById('alert-modal').classList.add('active');
}

function closeAlertModal() {
    document.getElementById('alert-modal').classList.remove('active');
}

function formatBytes(bytes) {
    if (!bytes || bytes === 0) return '-';
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return `${parseFloat((bytes / Math.pow(1024, i)).toFixed(1))} ${sizes[i]}`;
}

function formatDuration(ms) {
    if (!ms || ms === 0) return '-';
    const seconds = Math.floor(ms / 1000);
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);
    if (days > 0) return `${days}d ${hours % 24}h`;
    if (hours > 0) return `${hours}h ${minutes % 60}m`;
    if (minutes > 0) return `${minutes}m ${seconds % 60}s`;
    return `${seconds}s`;
}

function getStatusBadge(status) {
    const statusLower = status.toLowerCase();
    const iconNames = {
        'running': 'activity',
        'online': 'activity',
        'stopped': 'square',
        'errored': 'alertCircle',
        'starting': 'rotateCw',
        'launching': 'rotateCw',
    };
    const badgeClass = {
        'running': 'badge-online',
        'online': 'badge-online',
        'stopped': 'badge-stopped',
        'errored': 'badge-errored',
        'starting': 'badge-launching',
        'launching': 'badge-launching',
    };
    const iconName = iconNames[statusLower] || 'square';
    const badge = badgeClass[statusLower] || 'badge-stopped';
    return `<span class="badge ${badge}">${icon(iconName, 'icon-sm')}${status}</span>`;
}

async function loadProcesses() {
    const response = await fetchData('/processes');
    if (!response || !response.success || !response.data) return;

    const processes = response.data.processes;
    const tbody = document.getElementById('process-table-body');
    const emptyState = document.getElementById('empty-state');

    if (processes.length === 0) {
        tbody.innerHTML = '';
        emptyState.style.display = 'block';
        return;
    }

    emptyState.style.display = 'none';
    tbody.innerHTML = processes.map((p, index) => `
        <tr class="${index % 2 === 0 ? 'even' : ''} cursor-pointer" onclick="viewDetails('${escapeHtml(p.id)}')">
            <td onclick="event.stopPropagation()">
                <input type="checkbox" class="process-checkbox" value="${escapeHtml(p.id)}" onchange="updateProcessBatchActionsVisibility()">
            </td>
            <td>
                <div style="display: flex; align-items: center; gap: 0.5rem;">
                    ${icon('terminal', 'icon-sm', 2, 'hsl(var(--muted-foreground))')}
                    <span style="font-weight: 500;">${escapeHtml(p.name)}</span>
                </div>
            </td>
            <td>${getStatusBadge(p.status)}</td>
            <td class="mono">${p.pid || '-'}</td>
            <td>
                <div style="display: flex; align-items: center; gap: 0.5rem;">
                    ${icon('cpu', 'icon-sm', 2, 'hsl(var(--muted-foreground))')}
                    <span>${(p.cpu_percent || 0).toFixed(1)}%</span>
                </div>
            </td>
            <td>
                <div style="display: flex; align-items: center; gap: 0.5rem;">
                    ${icon('hardDrive', 'icon-sm', 2, 'hsl(var(--muted-foreground))')}
                    <span>${formatBytes(p.memory_bytes)}</span>
                </div>
            </td>
            <td>
                <div style="display: flex; align-items: center; gap: 0.5rem;">
                    ${icon('clock', 'icon-sm', 2, 'hsl(var(--muted-foreground))')}
                    <span>${formatDuration(p.uptime_ms)}</span>
                </div>
            </td>
            <td>${p.restart_count}</td>
            <td>
                <div class="action-btns">
                    ${p.status === 'stopped' || p.status === 'errored' ? `
                        <button class="btn-ghost" onclick="event.stopPropagation(); handleAction('start', '${escapeHtml(p.id)}')" title="Start">
                            ${icon('play', 'icon-sm', 2, 'hsl(142, 71%, 45%)')}
                        </button>
                    ` : `
                        <button class="btn-ghost" onclick="event.stopPropagation(); handleAction('stop', '${escapeHtml(p.id)}')" title="Stop">
                            ${icon('square', 'icon-sm', 2, 'hsl(45, 93%, 47%)')}
                        </button>
                        <button class="btn-ghost" onclick="event.stopPropagation(); handleAction('restart', '${escapeHtml(p.id)}')" title="Restart">
                            ${icon('rotateCw', 'icon-sm', 2, 'hsl(221, 83%, 53%)')}
                        </button>
                    `}
                    <button class="btn-ghost" onclick="event.stopPropagation(); showEditProcess('${escapeHtml(p.id)}')" title="Edit">
                        ${icon('settings', 'icon-sm', 2, 'hsl(215, 91%, 65%)')}
                    </button>
                    <button class="btn-ghost" onclick="event.stopPropagation(); showDeleteConfirm('${escapeHtml(p.id)}', '${escapeHtml(p.name)}')" title="Delete">
                        ${icon('trash2', 'icon-sm', 2, 'hsl(0, 84%, 60%)')}
                    </button>
                    <button class="btn-ghost" onclick="event.stopPropagation(); viewLogs('${escapeHtml(p.id)}', '${escapeHtml(p.name)}')" title="Logs">
                        ${icon('fileText', 'icon-sm', 2, 'hsl(262, 83%, 58%)')}
                    </button>
                </div>
            </td>
        </tr>
    `).join('');
}

async function loadStatus() {
    const response = await fetchData('/status');
    if (!response || !response.success || !response.data) return;

    const data = response.data;
    document.getElementById('stat-total').textContent = data.total_processes || 0;
    
    // Update footer with version and uptime
    if (data.version) {
        document.getElementById('footer-version').textContent = `v${data.version}`;
        document.getElementById('about-version').textContent = `v${data.version}`;
    }
    if (data.uptime_ms !== undefined) {
        document.getElementById('footer-uptime').textContent = formatDuration(data.uptime_ms);
    }

    const processesResponse = await fetchData('/processes');
    if (processesResponse && processesResponse.success && processesResponse.data) {
        const procs = processesResponse.data.processes;
        const running = procs.filter(p => p.status === 'running' || p.status === 'online').length;
        const stopped = procs.filter(p => p.status === 'stopped').length;
        const errored = procs.filter(p => p.status === 'errored').length;

        document.getElementById('stat-running').textContent = running;
        document.getElementById('stat-stopped').textContent = stopped;
        document.getElementById('stat-errored').textContent = errored;
    }
}

async function handleAction(action, id) {
    const response = await postData(`/processes/${id}/${action}`);
    if (response && response.success) refreshData();
}

async function restartAll() {
    showConfirm('Restart all processes?', async () => {
        const response = await fetchData('/processes');
        if (response && response.success && response.data) {
            for (const p of response.data.processes) {
                await postData(`/processes/${p.id}/restart`);
            }
        }
        refreshData();
    });
}

async function stopAll() {
    showConfirm('Stop all processes?', async () => {
        await postData('/processes/stop-all');
        refreshData();
    });
}

function viewDetails(id) {
    currentDetailProcess = id;
    document.getElementById('details-modal').classList.add('active');
    loadProcessDetails(id);
}

async function loadProcessDetails(id) {
    const response = await fetchData(`/processes/${id}`);
    if (!response || !response.success || !response.data) return;

    const p = response.data;
    document.getElementById('details-title').textContent = `Process: ${p.name}`;
    document.getElementById('details-description').textContent = `ID: ${p.id}`;

    // Overview Tab
    document.getElementById('tab-overview').innerHTML = `
        <div class="details-section">
            <div class="details-section-title">
                ${icon('server', 'icon')}
                Basic Information
            </div>
            <div class="details-grid">
                <div class="detail-item">
                    <span class="detail-label">Name</span>
                    <span class="detail-value">${escapeHtml(p.name)}</span>
                </div>
                <div class="detail-item">
                    <span class="detail-label">Status</span>
                    <span class="detail-value">${getStatusBadge(p.status)}</span>
                </div>
                <div class="detail-item">
                    <span class="detail-label">PID</span>
                    <span class="detail-value mono">${p.pid || '-'}</span>
                </div>
                <div class="detail-item">
                    <span class="detail-label">Restarts</span>
                    <span class="detail-value">${p.restart_count}</span>
                </div>
            </div>
        </div>
        <div class="section-separator"></div>
        <div class="details-section">
            <div class="details-section-title">
                ${icon('cpu', 'icon')}
                Resource Usage
            </div>
            <div class="details-grid">
                <div class="detail-item">
                    <span class="detail-label">CPU</span>
                    <span class="detail-value">${(p.cpu_percent || 0).toFixed(2)}%</span>
                </div>
                <div class="detail-item">
                    <span class="detail-label">Memory</span>
                    <span class="detail-value">${formatBytes(p.memory_bytes)}</span>
                </div>
            </div>
        </div>
        <div class="section-separator"></div>
        <div class="details-section">
            <div class="details-section-title">
                ${icon('clock', 'icon')}
                Runtime
            </div>
            <div class="detail-item">
                <span class="detail-label">Uptime</span>
                <span class="detail-value">${formatDuration(p.uptime_ms)}</span>
            </div>
        </div>
    `;

    // Configuration Tab
    document.getElementById('tab-config').innerHTML = `
        <div class="details-section">
            <div class="details-section-title">
                ${icon('settings', 'icon')}
                Startup Configuration
            </div>
            <div class="details-grid">
                ${p.cwd ? `
                <div class="detail-item">
                    <span class="detail-label">Working Directory</span>
                    <span class="detail-value mono">${escapeHtml(p.cwd)}</span>
                </div>
                ` : ''}
                ${p.command ? `
                <div class="detail-item">
                    <span class="detail-label">Command</span>
                    <span class="detail-value mono">${escapeHtml(p.command)}</span>
                </div>
                ` : ''}
                ${p.args && p.args.length > 0 ? `
                <div class="detail-item">
                    <span class="detail-label">Arguments</span>
                    <span class="detail-value mono">${escapeHtml(p.args.join(' '))}</span>
                </div>
                ` : ''}
                ${p.instances ? `
                <div class="detail-item">
                    <span class="detail-label">Instances</span>
                    <span class="detail-value">${p.instances}</span>
                </div>
                ` : ''}
                <div class="detail-item">
                    <span class="detail-label">Auto Restart</span>
                    <span class="detail-value">${p.config?.autorestart ? 'Enabled' : 'Disabled'}</span>
                </div>
                <div class="detail-item">
                    <span class="detail-label">Watch Mode</span>
                    <span class="detail-value">${p.config?.watch ? 'Enabled' : 'Disabled'}</span>
                </div>
                ${p.config?.max_memory_mb ? `
                <div class="detail-item">
                    <span class="detail-label">Max Memory Restart</span>
                    <span class="detail-value">${p.config.max_memory_mb}MB</span>
                </div>
                ` : ''}
                ${p.config?.env && Object.keys(p.config.env).length > 0 ? `
                <div class="detail-item" style="grid-column: 1 / -1;">
                    <span class="detail-label">Environment Variables</span>
                    <div class="log-viewer" style="max-height: 150px; margin-top: 0.5rem;">
                        ${Object.entries(p.config.env).map(([key, value]) => 
                            `<div class="log-line"><span style="color: hsl(var(--muted-foreground));">${escapeHtml(key)}=</span><span style="color: hsl(var(--primary));">${escapeHtml(value)}</span></div>`
                        ).join('')}
                    </div>
                </div>
                ` : ''}
            </div>
        </div>
    `;

    // Raw Data Tab
    document.getElementById('tab-raw').innerHTML = `
        <div class="details-section">
            <div class="details-section-title">
                ${icon('fileJson', 'icon')}
                Raw Process Data
            </div>
            <div class="code-block">
                <pre>${escapeHtml(JSON.stringify(p, null, 2))}</pre>
            </div>
        </div>
    `;

    // Actions Tab
    document.getElementById('tab-actions').innerHTML = `
        <div class="details-section">
            <div class="details-section-title">
                ${icon('zap', 'icon')}
                Process Actions
            </div>
            <p style="color: hsl(var(--muted-foreground)); font-size: 0.875rem; margin-bottom: 1rem;">
                Control the selected process with the following actions
            </p>
            <div style="display: flex; gap: 0.75rem; flex-wrap: wrap; margin-top: 1rem;">
                ${p.status === 'stopped' || p.status === 'errored' ? `
                    <button class="btn-primary" onclick="handleAction('start', '${escapeHtml(p.id)}'); closeModal('details-modal'); refreshData();" style="flex: 1; min-width: 120px;">
                        ${icon('play', 'icon-sm', 2, 'hsl(142, 71%, 45%)')}
                        Start
                    </button>
                ` : `
                    <button class="btn-secondary" onclick="handleAction('stop', '${escapeHtml(p.id)}'); closeModal('details-modal'); refreshData();" style="flex: 1; min-width: 120px;">
                        ${icon('square', 'icon-sm', 2, 'hsl(45, 93%, 47%)')}
                        Stop
                    </button>
                    <button class="btn-secondary" onclick="handleAction('restart', '${escapeHtml(p.id)}'); closeModal('details-modal'); refreshData();" style="flex: 1; min-width: 120px;">
                        ${icon('rotateCw', 'icon-sm', 2, 'hsl(221, 83%, 53%)')}
                        Restart
                    </button>
                `}
                <button class="btn-destructive" onclick="showDeleteConfirm('${escapeHtml(p.id)}', '${escapeHtml(p.name)}'); closeModal('details-modal');" style="flex: 1; min-width: 120px;">
                    ${icon('trash2', 'icon-sm', 2, 'hsl(0, 84%, 60%)')}
                    Delete
                </button>
            </div>
        </div>
    `;

    switchTab('overview');
}

function switchTab(tabName) {
    document.querySelectorAll('.tabs-trigger').forEach((t, i) => {
        t.classList.toggle('active', t.textContent.toLowerCase().includes(tabName));
    });
    document.querySelectorAll('.tabs-content').forEach(c => c.classList.remove('active'));
    document.getElementById(`tab-${tabName}`).classList.add('active');
}

async function viewLogs(id, name) {
    currentLogsProcess = id;
    currentLogsTab = 'stdout'; // Reset to stdout tab when opening logs
    document.getElementById('logs-title').textContent = 'Process Logs';
    document.getElementById('logs-description').textContent = `Viewing logs for: ${name} (ID: ${id})`;
    document.getElementById('logs-modal').classList.add('active');
    await refreshLogs();
    if (logsAutoRefresh) startLogsTimer();
}

async function refreshLogs() {
    if (!currentLogsProcess) return;
    
    const btn = document.getElementById('logs-refresh-btn');
    if (btn) btn.querySelector('svg').classList.add('animate-spin');
    
    const response = await fetchData(`/processes/${currentLogsProcess}/logs`);
    
    // Spin animation for 1 second
    await new Promise(resolve => setTimeout(resolve, 1000));
    if (btn) btn.querySelector('svg').classList.remove('animate-spin');

    const content = document.getElementById('logs-content');
    if (response && response.success && response.data && response.data.length > 0) {
        const stdout = response.data.filter(l => !l.is_error).map(l => l.message).join('\n');
        const stderr = response.data.filter(l => l.is_error).map(l => l.message).join('\n');
        const stdoutCount = response.data.filter(l => !l.is_error).length;
        const stderrCount = response.data.filter(l => l.is_error).length;
        
        content.innerHTML = `
            <div class="logs-tabs">
                <div class="logs-tab-list">
                    <button class="logs-tab-btn" data-tab="stdout" onclick="switchLogsTab('stdout')">
                        <div class="log-dot green"></div>
                        Stdout (${stdoutCount})
                    </button>
                    <button class="logs-tab-btn" data-tab="stderr" onclick="switchLogsTab('stderr')">
                        <div class="log-dot red"></div>
                        Stderr (${stderrCount})
                    </button>
                </div>
                <div id="stdout-tab" class="logs-tab-content">
                    <div class="log-viewer log-content info" style="height: 100%;">${escapeHtml(stdout)}</div>
                </div>
                <div id="stderr-tab" class="logs-tab-content">
                    <div class="log-viewer log-content error" style="height: 100%;">${escapeHtml(stderr)}</div>
                </div>
            </div>
        `;
        
        // Restore the previously active tab
        switchLogsTab(currentLogsTab);
        
        // Auto scroll to bottom after DOM is updated
        setTimeout(() => {
            const activeTab = document.querySelector('.logs-tab-content.active .log-viewer');
            if (activeTab) {
                activeTab.scrollTop = activeTab.scrollHeight;
            }
        }, 50);
    } else {
        content.innerHTML = '<div class="empty-state"><p>No logs available</p></div>';
    }
}

function switchLogsTab(tabName) {
    currentLogsTab = tabName; // Save current tab
    
    // Update tab buttons
    document.querySelectorAll('.logs-tab-btn').forEach(btn => {
        btn.classList.toggle('active', btn.dataset.tab === tabName);
    });
    
    // Update tab content
    document.querySelectorAll('.logs-tab-content').forEach(content => {
        content.classList.toggle('active', content.id === `${tabName}-tab`);
    });
    
    // Scroll to bottom of active tab
    setTimeout(() => {
        const activeTab = document.querySelector('.logs-tab-content.active .log-viewer');
        if (activeTab) {
            activeTab.scrollTop = activeTab.scrollHeight;
        }
    }, 0);
}

function toggleLogsAutoRefresh() {
    logsAutoRefresh = !logsAutoRefresh;
    document.getElementById('logs-auto-text').textContent = logsAutoRefresh ? 'On' : 'Off';
    
    // Update button style
    const btn = document.getElementById('logs-auto-btn');
    if (btn) {
        btn.classList.toggle('btn-primary', logsAutoRefresh);
        btn.classList.toggle('btn-outline', !logsAutoRefresh);
    }
    
    if (logsAutoRefresh) startLogsTimer();
    else stopLogsTimer();
}

function startLogsTimer() {
    stopLogsTimer();
    logsTimer = setInterval(refreshLogs, logsInterval * 1000);
}

function stopLogsTimer() {
    if (logsTimer) clearInterval(logsTimer);
}

function updateLogsInterval() {
    logsInterval = parseInt(document.getElementById('logs-interval').value);
    if (logsAutoRefresh) startLogsTimer();
}

function showAddProcess() {
    document.getElementById('add-modal').classList.add('active');
}

async function submitAddProcess() {
    const name = document.getElementById('add-name').value.trim();
    const commandInput = document.getElementById('add-command').value.trim();
    const cwd = document.getElementById('add-cwd').value.trim();
    const instances = parseInt(document.getElementById('add-instances').value) || 1;
    const maxMemory = document.getElementById('add-max-memory').value.trim();
    const envText = document.getElementById('add-env').value.trim();
    const autorestart = document.getElementById('add-autorestart').classList.contains('active');
    const watch = document.getElementById('add-watch').classList.contains('active');

    if (!name) {
        showError('Process name is required');
        return;
    }
    if (!commandInput) {
        showError('Command is required');
        return;
    }

    const env = {};
    if (envText) {
        envText.split('\n').forEach(line => {
            const [key, ...valueParts] = line.split('=');
            if (key.trim()) env[key.trim()] = valueParts.join('=').trim();
        });
    }

    // Parse command: split by whitespace (e.g., "npm run dev" -> ["npm", "run", "dev"])
    const parts = commandInput.split(/\s+/);
    const command = parts[0];
    const args = parts.slice(1);

    const response = await postData('/processes', {
        name,
        command,
        args,
        instances,
        cwd: cwd || undefined,
        env: Object.keys(env).length > 0 ? env : undefined,
        autorestart,
        watch,
        max_memory_mb: maxMemory ? parseInt(maxMemory) : undefined
    });

    if (response && response.success) {
        closeModal('add-modal');
        // Reset form
        document.getElementById('add-name').value = '';
        document.getElementById('add-command').value = '';
        document.getElementById('add-cwd').value = '';
        document.getElementById('add-instances').value = '1';
        document.getElementById('add-max-memory').value = '';
        document.getElementById('add-env').value = '';
        document.getElementById('add-autorestart').classList.add('active');
        document.getElementById('add-watch').classList.remove('active');
        refreshData();
    } else {
        showError('Failed to start process: ' + (response?.error || 'Unknown error'));
    }
}

// ============ Edit Process Functions ============

let currentEditProcessId = null;

async function showEditProcess(id) {
    currentEditProcessId = id;
    
    // Fetch process details
    const response = await fetchData(`/processes/${id}`);
    if (!response || !response.success || !response.data) {
        showError('Failed to load process details');
        return;
    }
    
    const p = response.data;
    const config = p.config || {};
    
    // Fill form fields
    document.getElementById('edit-id').value = id;
    document.getElementById('edit-name').value = p.name || '';
    // Show full command including args
    const fullCommand = config.command ? (config.args && config.args.length > 0 
        ? config.command + ' ' + config.args.join(' ') 
        : config.command) : '';
    document.getElementById('edit-command').value = fullCommand;
    document.getElementById('edit-cwd').value = config.cwd || '';
    document.getElementById('edit-instances').value = config.instances || 1;
    document.getElementById('edit-max-memory').value = config.max_memory_mb || 0;
    document.getElementById('edit-max-restarts').value = config.max_restarts || 15;
    
    // Environment variables
    if (config.env && Object.keys(config.env).length > 0) {
        const envLines = Object.entries(config.env).map(([key, value]) => `${key}=${value}`).join('\n');
        document.getElementById('edit-env').value = envLines;
    } else {
        document.getElementById('edit-env').value = '';
    }
    
    // Toggle states
    if (config.autorestart) {
        document.getElementById('edit-autorestart').classList.add('active');
    } else {
        document.getElementById('edit-autorestart').classList.remove('active');
    }
    
    if (config.watch) {
        document.getElementById('edit-watch').classList.add('active');
    } else {
        document.getElementById('edit-watch').classList.remove('active');
    }
    
    // Show modal
    document.getElementById('edit-modal').classList.add('active');
}

async function submitEditProcess() {
    const id = document.getElementById('edit-id').value;
    if (!id) {
        showError('Process ID is missing');
        return;
    }
    
    const name = document.getElementById('edit-name').value.trim();
    const commandInput = document.getElementById('edit-command').value.trim();
    const cwd = document.getElementById('edit-cwd').value.trim();
    const instances = parseInt(document.getElementById('edit-instances').value) || 1;
    const maxMemory = parseInt(document.getElementById('edit-max-memory').value) || 0;
    const maxRestarts = parseInt(document.getElementById('edit-max-restarts').value) || 15;
    const envText = document.getElementById('edit-env').value.trim();
    const autorestart = document.getElementById('edit-autorestart').classList.contains('active');
    const watch = document.getElementById('edit-watch').classList.contains('active');
    
    if (!name) {
        showError('Process name is required');
        return;
    }
    if (!commandInput) {
        showError('Command is required');
        return;
    }
    
    const env = {};
    if (envText) {
        envText.split('\n').forEach(line => {
            const [key, ...valueParts] = line.split('=');
            if (key.trim()) env[key.trim()] = valueParts.join('=').trim();
        });
    }
    
    // Parse command
    const parts = commandInput.split(/\s+/);
    const command = parts[0];
    const args = parts.slice(1);
    
    const response = await fetch(`/api/processes/${id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            name,
            command,
            args,
            instances,
            cwd: cwd || null,
            env: Object.keys(env).length > 0 ? env : null,
            autorestart,
            watch,
            max_memory_mb: maxMemory,
            max_restarts: maxRestarts
        })
    });
    
    const result = await response.json();
    if (result.success) {
        closeModal('edit-modal');
        refreshData();
    } else {
        showError('Failed to update process: ' + (result.error || 'Unknown error'));
    }
}

function showDeleteConfirm(id, name) {
    deleteProcessId = id;
    document.getElementById('delete-process-name').textContent = name;
    document.getElementById('delete-modal').classList.add('active');
}

async function confirmDelete() {
    if (deleteProcessId) {
        await postData(`/processes/${deleteProcessId}/delete`);
        closeModal('delete-modal');
        refreshData();
    }
}

function showAbout() {
    document.getElementById('about-modal').classList.add('active');
}

function closeModal(modalId) {
    document.getElementById(modalId).classList.remove('active');
    if (modalId === 'logs-modal') {
        stopLogsTimer();
        logsAutoRefresh = false;
        currentLogsProcess = null;
    }
    if (modalId === 'details-modal') {
        currentDetailProcess = null;
    }
}

function toggleAutoRefresh() {
    autoRefresh = !autoRefresh;
    document.getElementById('auto-refresh-text').textContent = autoRefresh ? 'Auto' : 'Manual';
    document.getElementById('auto-refresh-btn').classList.toggle('btn-primary', autoRefresh);
    document.getElementById('auto-refresh-btn').classList.toggle('btn-outline', !autoRefresh);
    
    // Add/remove spin animation to the icon
    const icon = document.getElementById('auto-refresh-icon');
    if (icon) {
        if (autoRefresh) {
            icon.classList.add('animate-spin');
        } else {
            icon.classList.remove('animate-spin');
        }
    }
    
    if (autoRefresh) {
        refreshInterval = setInterval(refreshData, 5000);
    } else {
        clearInterval(refreshInterval);
    }
}

function refreshData() {
    loadProcesses();
    loadStatus();
}

function escapeHtml(text) {
    if (!text) return '';
    const map = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#039;' };
    return text.toString().replace(/[&<>"']/g, m => map[m]);
}

// Initialize
initTheme();
// Restore saved tab
switchMainTab(currentMainTab);
refreshData();
refreshInterval = setInterval(refreshData, 5000);

// Set initial state for auto-refresh button
const autoRefreshBtn = document.getElementById('auto-refresh-btn');
if (autoRefreshBtn) {
    autoRefreshBtn.classList.toggle('btn-primary', autoRefresh);
    autoRefreshBtn.classList.toggle('btn-outline', !autoRefresh);
}

// Add spin animation to auto-refresh icon on page load
const autoRefreshIcon = document.getElementById('auto-refresh-icon');
if (autoRefreshIcon && autoRefresh) {
    autoRefreshIcon.classList.add('animate-spin');
}

// Set initial state for logs auto-refresh button
const logsAutoBtn = document.getElementById('logs-auto-btn');
if (logsAutoBtn) {
    logsAutoBtn.classList.toggle('btn-primary', logsAutoRefresh);
    logsAutoBtn.classList.toggle('btn-outline', !logsAutoRefresh);
}

// Keyboard shortcuts
document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
        closeModal('details-modal');
        closeModal('logs-modal');
        closeModal('add-modal');
        closeModal('edit-modal');
        closeModal('edit-schedule-modal');
        closeModal('delete-modal');
        closeModal('delete-schedule-modal');
        closeConfirmModal();
        closeAlertModal();
    }
});

// ============ Main Tab Navigation ============

function switchMainTab(tabName) {
    console.log('switchMainTab called:', tabName);
    currentMainTab = tabName;
    localStorage.setItem('currentMainTab', tabName);

    // Update tab buttons
    document.getElementById('tab-btn-processes').classList.toggle('active', tabName === 'processes');
    document.getElementById('tab-btn-schedules').classList.toggle('active', tabName === 'schedules');

    // Update content
    document.getElementById('processes-content').style.display = tabName === 'processes' ? 'block' : 'none';
    document.getElementById('schedules-content').style.display = tabName === 'schedules' ? 'block' : 'none';

    // Load data for the active tab
    if (tabName === 'processes') {
        loadProcesses();
    } else if (tabName === 'schedules') {
        loadSchedules();
    }
}

// ============ Schedule Functions ============

function getScheduleStatusBadge(status) {
    const statusLower = status.toLowerCase();
    const iconNames = {
        'active': 'activity',
        'paused': 'pause',
        'completed': 'check',
        'error': 'alertCircle',
    };
    const badgeClass = {
        'active': 'badge-online',
        'paused': 'badge-stopped',
        'completed': 'badge-stopped',
        'error': 'badge-errored',
    };
    const iconName = iconNames[statusLower] || 'pause';
    const badge = badgeClass[statusLower] || 'badge-stopped';
    return `<span class="badge ${badge}">${icon(iconName, 'icon-sm')}${status}</span>`;
}

async function loadSchedules() {
    const response = await fetchData('/schedules');
    if (!response || !response.success || !response.data) return;

    const schedules = response.data.schedules;
    const tbody = document.getElementById('schedule-table-body');
    const emptyState = document.getElementById('schedule-empty-state');

    if (schedules.length === 0) {
        tbody.innerHTML = '';
        emptyState.style.display = 'block';
        return;
    }

    emptyState.style.display = 'none';
    tbody.innerHTML = schedules.map((s, index) => `
        <tr class="${index % 2 === 0 ? 'even' : ''}">
            <td>
                <input type="checkbox" class="schedule-checkbox" value="${escapeHtml(s.id)}" onchange="updateBatchActionsVisibility()">
            </td>
            <td style="white-space: nowrap;">
                <div style="display: flex; align-items: center; gap: 0.5rem;" title="${s.description ? escapeHtml(s.description) : escapeHtml(s.name)}">
                    ${icon('clock', 'icon-sm', 2, 'hsl(var(--muted-foreground))')}
                    <div style="display: flex; flex-direction: column; min-width: 0;">
                        <span style="font-weight: 500;">${escapeHtml(s.name)}</span>
                        ${s.description ? `<span style="font-size: 0.75rem; color: hsl(var(--muted-foreground)); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 180px;">${escapeHtml(s.description)}</span>` : ''}
                    </div>
                </div>
            </td>
            <td>${getScheduleStatusBadge(s.status)}</td>
            <td class="mono" style="white-space: nowrap;">${escapeHtml(s.schedule_type)}</td>
            <td>
                <div style="display: flex; flex-direction: column; gap: 0.25rem;">
                    <span class="badge" style="background: hsl(262, 83%, 58%, 0.1); color: hsl(262, 83%, 58%); width: fit-content;">${escapeHtml(s.action)}</span>
                    <span class="mono" style="font-size: 0.75rem; color: hsl(var(--muted-foreground)); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 150px;" title="${s.action === 'execute' ? (s.command ? escapeHtml(s.command + (s.args && s.args.length > 0 ? ' ' + s.args.join(' ') : '')) : '-') : (s.target_process ? escapeHtml(s.target_process) : '-')}">
                        ${s.action === 'execute' ? (s.command ? escapeHtml(s.command + (s.args && s.args.length > 0 ? ' ' + s.args.join(' ') : '')) : '-') : (s.target_process ? escapeHtml(s.target_process) : '-')}
                    </span>
                </div>
            </td>
            <td>${s.next_run ? escapeHtml(s.next_run) : '-'}</td>
            <td>${s.last_run ? escapeHtml(s.last_run) : '-'}</td>
            <td>
                <div style="display: flex; align-items: center; gap: 0.5rem;">
                    <span style="color: hsl(142, 71%, 45%);">${s.success_count}</span>
                    <span style="color: hsl(var(--muted-foreground));">/</span>
                    <span style="color: hsl(0, 84%, 60%);">${s.fail_count}</span>
                </div>
            </td>
            <td>
                <div class="action-btns">
                    ${s.status === 'active' ? `
                        <button class="btn-ghost" onclick="handleScheduleAction('pause', '${escapeHtml(s.id)}')" title="Pause">
                            ${icon('pause', 'icon-sm', 2, 'hsl(45, 93%, 47%)')}
                        </button>
                    ` : `
                        <button class="btn-ghost" onclick="handleScheduleAction('resume', '${escapeHtml(s.id)}')" title="Resume">
                            ${icon('play', 'icon-sm', 2, 'hsl(142, 71%, 45%)')}
                        </button>
                    `}
                    <button class="btn-ghost" onclick="showEditSchedule('${escapeHtml(s.id)}')" title="Edit">
                        ${icon('settings', 'icon-sm', 2, 'hsl(215, 91%, 65%)')}
                    </button>
                    <button class="btn-ghost" onclick="showDeleteScheduleConfirm('${escapeHtml(s.id)}', '${escapeHtml(s.name)}')" title="Delete">
                        ${icon('trash2', 'icon-sm', 2, 'hsl(0, 84%, 60%)')}
                    </button>
                </div>
            </td>
        </tr>
    `).join('');
}

async function refreshSchedules() {
    await loadSchedules();
}

async function handleScheduleAction(action, id) {
    const response = await postData(`/schedules/${id}/${action}`);
    if (response && response.success) {
        refreshSchedules();
    }
}

// ============ Batch Schedule Operations ============

function getSelectedScheduleIds() {
    const checkboxes = document.querySelectorAll('.schedule-checkbox:checked');
    return Array.from(checkboxes).map(cb => cb.value);
}

function updateBatchActionsVisibility() {
    const selectedIds = getSelectedScheduleIds();
    const batchActionsDiv = document.getElementById('schedule-batch-actions');
    if (batchActionsDiv) {
        batchActionsDiv.style.display = selectedIds.length > 0 ? 'flex' : 'none';
    }
}

function toggleSelectAllSchedules() {
    const selectAllCheckbox = document.getElementById('schedule-select-all');
    const checkboxes = document.querySelectorAll('.schedule-checkbox');
    checkboxes.forEach(cb => {
        cb.checked = selectAllCheckbox.checked;
    });
    updateBatchActionsVisibility();
}

async function batchPauseSchedules() {
    const ids = getSelectedScheduleIds();
    if (ids.length === 0) {
        showError('Please select at least one schedule to pause');
        return;
    }

    showConfirm(`Pause ${ids.length} selected schedule(s)?`, async () => {
        const response = await postData('/schedules/batch/pause', { ids });
        if (response && response.success) {
            // Clear all checkboxes
            document.querySelectorAll('.schedule-checkbox').forEach(cb => cb.checked = false);
            document.getElementById('schedule-select-all').checked = false;
            updateBatchActionsVisibility();
            refreshSchedules();
        }
    });
}

async function batchResumeSchedules() {
    const ids = getSelectedScheduleIds();
    if (ids.length === 0) {
        showError('Please select at least one schedule to resume');
        return;
    }

    showConfirm(`Resume ${ids.length} selected schedule(s)?`, async () => {
        const response = await postData('/schedules/batch/resume', { ids });
        if (response && response.success) {
            // Clear all checkboxes
            document.querySelectorAll('.schedule-checkbox').forEach(cb => cb.checked = false);
            document.getElementById('schedule-select-all').checked = false;
            updateBatchActionsVisibility();
            refreshSchedules();
        }
    });
}

async function batchDeleteSchedules() {
    const ids = getSelectedScheduleIds();
    if (ids.length === 0) {
        showError('Please select at least one schedule to delete');
        return;
    }

    showConfirm(`Delete ${ids.length} selected schedule(s)? This action cannot be undone.`, async () => {
        const response = await postData('/schedules/batch/delete', { ids });
        if (response && response.success) {
            // Clear all checkboxes
            document.querySelectorAll('.schedule-checkbox').forEach(cb => cb.checked = false);
            document.getElementById('schedule-select-all').checked = false;
            updateBatchActionsVisibility();
            refreshSchedules();
        }
    });
}

// ============ Edit Schedule Functions ============

function toggleEditScheduleFields() {
    const type = document.getElementById('edit-schedule-type').value;
    const label = document.getElementById('edit-schedule-value-label');
    const placeholder = document.getElementById('edit-schedule-value');
    const help = document.getElementById('edit-schedule-value-help');

    if (type === 'cron') {
        label.textContent = 'Cron Expression *';
        placeholder.placeholder = '0 0 2 * * *';
        help.textContent = '6-field cron: 秒 分 时 天 月 周 (例如: 0 0 2 * * * = 每天凌晨2点)';
    } else if (type === 'interval') {
        label.textContent = 'Interval (seconds) *';
        placeholder.placeholder = '60';
        help.textContent = '执行间隔（秒），例如: 60 = 每分钟执行一次';
    } else if (type === 'once') {
        label.textContent = 'DateTime *';
        placeholder.placeholder = '2026-03-10T10:00:00';
        help.textContent = '执行时间，格式: YYYY-MM-DDTHH:MM:SS';
    }
}

function toggleEditActionFields() {
    const action = document.getElementById('edit-schedule-action').value;
    const processGroup = document.getElementById('edit-process-name-group');
    const executeFields = document.getElementById('edit-execute-fields');

    if (action === 'execute') {
        processGroup.style.display = 'none';
        executeFields.style.display = 'block';
    } else {
        processGroup.style.display = 'block';
        executeFields.style.display = 'none';
    }
}

async function showEditSchedule(id) {
    // Fetch schedule details
    const response = await fetchData(`/schedules/${id}`);
    if (!response || !response.success || !response.data) {
        showError('Failed to load schedule details');
        return;
    }
    
    const s = response.data;
    
    // Fill form fields
    document.getElementById('edit-schedule-id').value = id;
    document.getElementById('edit-schedule-name').value = s.name || '';
    document.getElementById('edit-schedule-description').value = s.description || '';
    
    // Parse schedule type and value
    let scheduleType = 'cron';
    let scheduleValue = '';
    if (s.schedule_type.startsWith('cron(')) {
        scheduleType = 'cron';
        scheduleValue = s.schedule_type.replace('cron(', '').replace(')', '');
    } else if (s.schedule_type.startsWith('interval(')) {
        scheduleType = 'interval';
        scheduleValue = s.schedule_type.replace('interval(', '').replace(')', '').replace('s', '');
    } else if (s.schedule_type.startsWith('once(')) {
        scheduleType = 'once';
        scheduleValue = s.schedule_type.replace('once(', '').replace(')', '');
    }
    
    document.getElementById('edit-schedule-type').value = scheduleType;
    document.getElementById('edit-schedule-value').value = scheduleValue;
    toggleEditScheduleFields();
    
    // Action
    document.getElementById('edit-schedule-action').value = s.action || 'start';
    document.getElementById('edit-schedule-process').value = s.target_process || '';
    
    // Execute fields
    if (s.action === 'execute') {
        // Parse command and args from schedule info if available
        document.getElementById('edit-schedule-action').value = 'execute';
        document.getElementById('edit-process-name-group').style.display = 'none';
        document.getElementById('edit-execute-fields').style.display = 'block';
        // Fill command and args
        document.getElementById('edit-schedule-command').value = s.command || '';
        document.getElementById('edit-schedule-args').value = s.args && Array.isArray(s.args) ? s.args.join(', ') : '';
    } else {
        document.getElementById('edit-process-name-group').style.display = 'block';
        document.getElementById('edit-execute-fields').style.display = 'none';
        document.getElementById('edit-schedule-command').value = '';
        document.getElementById('edit-schedule-args').value = '';
    }
    
    toggleEditActionFields();
    
    // Show modal
    document.getElementById('edit-schedule-modal').classList.add('active');
}

async function submitEditSchedule() {
    const id = document.getElementById('edit-schedule-id').value;
    if (!id) {
        showError('Schedule ID is missing');
        return;
    }
    
    const name = document.getElementById('edit-schedule-name').value.trim();
    const description = document.getElementById('edit-schedule-description').value.trim();
    const scheduleType = document.getElementById('edit-schedule-type').value;
    const scheduleValue = document.getElementById('edit-schedule-value').value.trim();
    const action = document.getElementById('edit-schedule-action').value;
    const processName = document.getElementById('edit-schedule-process').value.trim();
    const command = document.getElementById('edit-schedule-command').value.trim();
    const argsInput = document.getElementById('edit-schedule-args').value.trim();
    
    if (!name) {
        showError('Schedule name is required');
        return;
    }
    if (!scheduleValue) {
        showError('Schedule value is required');
        return;
    }
    
    const args = argsInput ? argsInput.split(',').map(a => a.trim()) : [];
    
    const data = {
        name,
        description: description || null,
        schedule_type: scheduleType,
        schedule_value: scheduleValue,
        action,
        process_name: action !== 'execute' && processName ? processName : null,
        command: action === 'execute' && command ? command : null,
        args: action === 'execute' && args.length > 0 ? args : null,
    };
    
    const response = await fetch(`/api/schedules/${id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(data)
    });
    
    const result = await response.json();
    if (result.success) {
        closeModal('edit-schedule-modal');
        refreshSchedules();
    } else {
        showError('Failed to update schedule: ' + (result.error || 'Unknown error'));
    }
}

function showDeleteScheduleConfirm(id, name) {
    deleteScheduleId = id;
    document.getElementById('delete-schedule-name').textContent = name;
    document.getElementById('delete-schedule-modal').classList.add('active');
}

async function confirmDeleteSchedule() {
    if (deleteScheduleId) {
        await postData(`/schedules/${deleteScheduleId}/delete`);
        closeModal('delete-schedule-modal');
        refreshSchedules();
    }
}

// Override refreshData to also refresh schedules
const originalRefreshData = refreshData;
refreshData = function() {
    // Ensure correct content is visible based on current tab
    document.getElementById('processes-content').style.display = currentMainTab === 'processes' ? 'block' : 'none';
    document.getElementById('schedules-content').style.display = currentMainTab === 'schedules' ? 'block' : 'none';
    
    // Update tab buttons
    document.getElementById('tab-btn-processes').classList.toggle('active', currentMainTab === 'processes');
    document.getElementById('tab-btn-schedules').classList.toggle('active', currentMainTab === 'schedules');
    
    // Load data based on current tab
    if (currentMainTab === 'processes') {
        loadProcesses();
        loadStatus();
    } else if (currentMainTab === 'schedules') {
        loadSchedules();
    }
};

// ============ Add Schedule Functions ============

function showAddSchedule() {
    // Reset form
    document.getElementById('add-schedule-name').value = '';
    document.getElementById('add-schedule-description').value = '';
    document.getElementById('add-schedule-type').value = 'cron';
    document.getElementById('add-schedule-value').value = '';
    document.getElementById('add-schedule-action').value = 'start';
    document.getElementById('add-schedule-process').value = '';
    document.getElementById('add-schedule-command').value = '';
    document.getElementById('add-schedule-args').value = '';
    
    // Force update the displayed values
    const scheduleTypeSelect = document.getElementById('add-schedule-type');
    const scheduleActionSelect = document.getElementById('add-schedule-action');
    scheduleTypeSelect.value = 'cron';
    scheduleActionSelect.value = 'start';
    
    toggleScheduleFields();
    toggleActionFields();
    
    document.getElementById('add-schedule-modal').classList.add('active');
}

function toggleScheduleFields() {
    const type = document.getElementById('add-schedule-type').value;
    const label = document.getElementById('schedule-value-label');
    const placeholder = document.getElementById('add-schedule-value');
    const help = document.getElementById('schedule-value-help');
    
    if (type === 'cron') {
        label.textContent = 'Cron Expression *';
        placeholder.placeholder = '0 0 2 * * *';
        help.textContent = '6-field cron: 秒 分 时 天 月 周 (例如: 0 0 2 * * * = 每天凌晨2点)';
    } else if (type === 'interval') {
        label.textContent = 'Interval (seconds) *';
        placeholder.placeholder = '60';
        help.textContent = '执行间隔（秒），例如: 60 = 每分钟执行一次';
    } else if (type === 'once') {
        label.textContent = 'DateTime *';
        placeholder.placeholder = '2026-03-10T10:00:00';
        help.textContent = '执行时间，格式: YYYY-MM-DDTHH:MM:SS';
    }
}

function toggleActionFields() {
    const action = document.getElementById('add-schedule-action').value;
    const processGroup = document.getElementById('process-name-group');
    const executeFields = document.getElementById('execute-fields');
    
    if (action === 'execute') {
        processGroup.style.display = 'none';
        executeFields.style.display = 'block';
    } else {
        processGroup.style.display = 'block';
        executeFields.style.display = 'none';
    }
}

async function submitAddSchedule() {
    const name = document.getElementById('add-schedule-name').value.trim();
    const description = document.getElementById('add-schedule-description').value.trim();
    const scheduleType = document.getElementById('add-schedule-type').value;
    const scheduleValue = document.getElementById('add-schedule-value').value.trim();
    const action = document.getElementById('add-schedule-action').value;
    const processName = document.getElementById('add-schedule-process').value.trim();
    const command = document.getElementById('add-schedule-command').value.trim();
    const argsInput = document.getElementById('add-schedule-args').value.trim();
    
    if (!name) {
        showError('Schedule name is required');
        return;
    }
    if (!scheduleValue) {
        showError('Schedule value is required');
        return;
    }
    
    const args = argsInput ? argsInput.split(',').map(a => a.trim()) : [];
    
    const data = {
        name,
        description: description || null,
        schedule_type: scheduleType,
        schedule_value: scheduleValue,
        action,
        process_name: action !== 'execute' && processName ? processName : null,
        command: action === 'execute' && command ? command : null,
        args: action === 'execute' && args.length > 0 ? args : null,
        enabled: true
    };
    
    const response = await postData('/schedules', data);

    if (response && response.success) {
        closeModal('add-schedule-modal');
        refreshSchedules();
    } else {
        showError('Failed to create schedule: ' + (response?.error || 'Unknown error'));
    }
}

// ============ Batch Process Operations ============

function getSelectedProcessIds() {
    const checkboxes = document.querySelectorAll('.process-checkbox:checked');
    return Array.from(checkboxes).map(cb => cb.value);
}

function updateProcessBatchActionsVisibility() {
    const selectedIds = getSelectedProcessIds();
    const batchActionsDiv = document.getElementById('process-batch-actions');
    if (batchActionsDiv) {
        batchActionsDiv.style.display = selectedIds.length > 0 ? 'flex' : 'none';
    }
}

function toggleSelectAllProcesses() {
    const selectAllCheckbox = document.getElementById('process-select-all');
    const checkboxes = document.querySelectorAll('.process-checkbox');
    checkboxes.forEach(cb => {
        cb.checked = selectAllCheckbox.checked;
    });
    updateProcessBatchActionsVisibility();
}

async function batchStopProcesses() {
    const ids = getSelectedProcessIds();
    if (ids.length === 0) {
        showError('Please select at least one process to stop');
        return;
    }

    showConfirm(`Stop ${ids.length} selected process(es)?`, async () => {
        for (const id of ids) {
            await postData(`/processes/${id}/stop`);
        }
        // Clear all checkboxes
        document.querySelectorAll('.process-checkbox').forEach(cb => cb.checked = false);
        document.getElementById('process-select-all').checked = false;
        updateProcessBatchActionsVisibility();
        refreshData();
    });
}

async function batchRestartProcesses() {
    const ids = getSelectedProcessIds();
    if (ids.length === 0) {
        showError('Please select at least one process to restart');
        return;
    }

    showConfirm(`Restart ${ids.length} selected process(es)?`, async () => {
        for (const id of ids) {
            await postData(`/processes/${id}/restart`);
        }
        // Clear all checkboxes
        document.querySelectorAll('.process-checkbox').forEach(cb => cb.checked = false);
        document.getElementById('process-select-all').checked = false;
        updateProcessBatchActionsVisibility();
        refreshData();
    });
}

async function batchDeleteProcesses() {
    const ids = getSelectedProcessIds();
    if (ids.length === 0) {
        showError('Please select at least one process to delete');
        return;
    }

    showConfirm(`Delete ${ids.length} selected process(es)? This action cannot be undone.`, async () => {
        for (const id of ids) {
            await postData(`/processes/${id}/delete`);
        }
        // Clear all checkboxes
        document.querySelectorAll('.process-checkbox').forEach(cb => cb.checked = false);
        document.getElementById('process-select-all').checked = false;
        updateProcessBatchActionsVisibility();
        refreshData();
    });
}
