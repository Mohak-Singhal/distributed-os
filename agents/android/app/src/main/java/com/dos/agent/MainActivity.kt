package com.dos.agent

import android.content.BroadcastReceiver
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.SharedPreferences
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.view.HapticFeedbackConstants
import android.view.View
import android.view.animation.RotateAnimation
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.interpolator.view.animation.FastOutSlowInInterpolator
import androidx.lifecycle.lifecycleScope
import androidx.localbroadcastmanager.content.LocalBroadcastManager
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.bottomnavigation.BottomNavigationView
import com.google.android.material.button.MaterialButton
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : AppCompatActivity() {

    // ── Devices Tab ──────────────────────────────────────────
    private lateinit var receiveLogo: ImageView
    private lateinit var tvReceiveDeviceName: TextView
    private lateinit var tvReceiveIp: TextView
    private lateinit var tvReceiveStatus: TextView
    private lateinit var activeTransfersPanel: View
    private lateinit var rvActiveTransfers: RecyclerView
    private lateinit var historyPanel: View
    private lateinit var rvTransferHistory: RecyclerView
    private lateinit var btnClearHistory: MaterialButton
    private lateinit var layoutReceive: ScrollView

    // ── Transfer Tab ─────────────────────────────────────────
    private lateinit var layoutSend: ScrollView
    private lateinit var btnPhotos: View
    private lateinit var btnFiles: View
    private lateinit var btnText: View
    private lateinit var cardFileQueue: View
    private lateinit var btnClearFiles: View
    private lateinit var tvFileCount: TextView
    private lateinit var tvFileTotalSize: TextView
    private lateinit var fileThumbnailList: View
    private lateinit var thumbnailContainer: LinearLayout
    private lateinit var btnEditFiles: MaterialButton
    private lateinit var btnAddFiles: MaterialButton
    private lateinit var tvNearbyLabel: TextView
    private lateinit var btnRefreshScan: View
    private lateinit var btnManualIp: View
    private lateinit var scanningDot: View
    private lateinit var rvDiscoveredDevices: RecyclerView
    private lateinit var tvNoDevices: TextView
    private lateinit var btnTroubleshoot: MaterialButton
    private lateinit var tvHelpText: TextView

    // ── Settings Tab ─────────────────────────────────────────
    private lateinit var layoutSettings: ScrollView
    private lateinit var rowTheme: View
    private lateinit var rowColorMode: View
    private lateinit var switchAutoAccept: com.google.android.material.switchmaterial.SwitchMaterial
    private lateinit var switchEncryption: com.google.android.material.switchmaterial.SwitchMaterial
    private lateinit var rowPinCode: View
    private lateinit var rowDeviceName: View
    private lateinit var rowSaveLocation: View
    private lateinit var switchZip: com.google.android.material.switchmaterial.SwitchMaterial
    private lateinit var rowVersion: View
    private lateinit var rowClearHistory: View
    private lateinit var rowLicenses: View
    private lateinit var tvThemeValue: TextView
    private lateinit var tvColorModeValue: TextView
    private lateinit var tvPinCodeValue: TextView
    private lateinit var tvDeviceNameValue: TextView
    private lateinit var tvSaveLocationValue: TextView
    private lateinit var tvVersionValue: TextView
    private lateinit var tvClearHistoryCount: TextView

    // ── Shared ───────────────────────────────────────────────
    private lateinit var bottomNavigation: BottomNavigationView

    // ── State ────────────────────────────────────────────────
    private val selectedUris = mutableListOf<Uri>()
    private val fileSizeCache = mutableMapOf<String, Long>()
    private var pendingText: String? = null
    private var selectedDeviceHost: String? = null
    private var currentTab = R.id.menu_transfer
    private val settingsPrefs: SharedPreferences by lazy {
        getSharedPreferences("pdos_settings", Context.MODE_PRIVATE)
    }

    // ── Managers ─────────────────────────────────────────────
    private val discoveryManager = DeviceDiscoveryManager { peers ->
        deviceAdapter.submitList(peers)
        updateDeviceListVisibility(peers.isNotEmpty())
    }
    private lateinit var deviceAdapter: DeviceListTileAdapter
    private lateinit var transferAdapter: TransferListAdapter
    private lateinit var historyAdapter: TransferHistoryAdapter

    // ── Receivers ────────────────────────────────────────────
    private val progressReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val transfers = FileTransferManager.getActiveTransfers()
            transferAdapter.submitList(transfers)
            updateTransfersPanelVisibility(transfers.isNotEmpty())
        }
    }

    private val peerReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            discoveryManager.addOrUpdate(
                name = intent.getStringExtra("name") ?: "",
                host = intent.getStringExtra("host") ?: "",
                port = intent.getIntExtra("port", 0),
                platform = intent.getStringExtra("platform") ?: ""
            )
        }
    }

    private val peerLostReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            discoveryManager.remove(intent.getStringExtra("host") ?: "")
        }
    }

    // ── Pickers ──────────────────────────────────────────────
    private val photoPickerLauncher = if (Build.VERSION.SDK_INT >= 33) {
        registerForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(10)) { uris ->
            uris?.let { addFiles(it) }
        }
    } else {
        registerForActivityResult(ActivityResultContracts.GetMultipleContents()) { uris ->
            uris?.let { addFiles(it) }
        }
    }

    private val filePickerLauncher = registerForActivityResult(ActivityResultContracts.GetMultipleContents()) { uris ->
        uris?.let { addFiles(it) }
    }

    // ═════════════════════════════════════════════════════════
    //  LIFECYCLE
    // ═════════════════════════════════════════════════════════

    override fun onCreate(savedInstanceState: Bundle?) {
        if (Build.VERSION.SDK_INT >= 31) {
            DynamicColorsCompat.applyToActivity(this)
        }
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        setContentView(R.layout.activity_main)
        ViewCompat.setOnApplyWindowInsetsListener(findViewById(R.id.root_container)) { view, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            insets
        }

        bindViews()
        setupNodeService()
        setupPeerList()
        setupClickObservers()
        loadQueueState()
        updateQueueUI()
        handleIntent(intent)
    }

    override fun onNewIntent(intent: Intent?) {
        super.onNewIntent(intent)
        setIntent(intent)
        intent?.let { handleIntent(it) }
    }

    override fun onResume() {
        super.onResume()
        val pf = IntentFilter("PDOS_PEER_FOUND")
        val pl = IntentFilter("PDOS_PEER_LOST")
        LocalBroadcastManager.getInstance(this).registerReceiver(peerReceiver, pf)
        LocalBroadcastManager.getInstance(this).registerReceiver(peerLostReceiver, pl)
        LocalBroadcastManager.getInstance(this).registerReceiver(
            progressReceiver,
            IntentFilter(FileTransferManager.ACTION_PROGRESS)
        )
        val transfers = FileTransferManager.getActiveTransfers()
        transferAdapter.submitList(transfers)
        updateTransfersPanelVisibility(transfers.isNotEmpty())
        updateLocalNetworkInfo()
        updateHistory()
        updateSettingsValues()
    }

    override fun onPause() {
        super.onPause()
        LocalBroadcastManager.getInstance(this).unregisterReceiver(peerReceiver)
        LocalBroadcastManager.getInstance(this).unregisterReceiver(peerLostReceiver)
        LocalBroadcastManager.getInstance(this).unregisterReceiver(progressReceiver)
    }

    // ═════════════════════════════════════════════════════════
    //  SETUP
    // ═════════════════════════════════════════════════════════

    private fun bindViews() {
        layoutSend = findViewById(R.id.layoutSend)
        layoutReceive = findViewById(R.id.layoutReceive)
        layoutSettings = findViewById(R.id.layoutSettings)
        bottomNavigation = findViewById(R.id.bottomNavigation)

        // Send tab
        btnPhotos = findViewById(R.id.btnPhotos)
        btnFiles = findViewById(R.id.btnFiles)
        btnText = findViewById(R.id.btnText)
        cardFileQueue = findViewById(R.id.cardFileQueue)
        btnClearFiles = findViewById(R.id.btnClearFiles)
        tvFileCount = findViewById(R.id.tvFileCount)
        tvFileTotalSize = findViewById(R.id.tvFileTotalSize)
        fileThumbnailList = findViewById(R.id.fileThumbnailList)
        thumbnailContainer = findViewById(R.id.thumbnailContainer)
        btnEditFiles = findViewById(R.id.btnEditFiles)
        btnAddFiles = findViewById(R.id.btnAddFiles)
        tvNearbyLabel = findViewById(R.id.tvNearbyLabel)
        btnRefreshScan = findViewById(R.id.btnRefreshScan)
        btnManualIp = findViewById(R.id.btnManualIp)
        scanningDot = findViewById(R.id.scanningDot)
        rvDiscoveredDevices = findViewById(R.id.rvDiscoveredDevices)
        tvNoDevices = findViewById(R.id.tvNoDevices)
        btnTroubleshoot = findViewById(R.id.btnTroubleshoot)
        tvHelpText = findViewById(R.id.tvHelpText)

        // Receive tab
        receiveLogo = findViewById(R.id.receiveLogo)
        tvReceiveDeviceName = findViewById(R.id.tvReceiveDeviceName)
        tvReceiveIp = findViewById(R.id.tvReceiveIp)
        tvReceiveStatus = findViewById(R.id.tvReceiveStatus)
        activeTransfersPanel = findViewById(R.id.activeTransfersPanel)
        rvActiveTransfers = findViewById(R.id.rvActiveTransfers)
        historyPanel = findViewById(R.id.historyPanel)
        rvTransferHistory = findViewById(R.id.rvTransferHistory)
        btnClearHistory = findViewById(R.id.btnClearHistory)

        // Settings tab
        rowTheme = findViewById(R.id.rowTheme)
        rowColorMode = findViewById(R.id.rowColorMode)
        switchAutoAccept = findViewById(R.id.switchAutoAccept)
        switchEncryption = findViewById(R.id.switchEncryption)
        rowPinCode = findViewById(R.id.rowPinCode)
        rowDeviceName = findViewById(R.id.rowDeviceName)
        rowSaveLocation = findViewById(R.id.rowSaveLocation)
        switchZip = findViewById(R.id.switchZip)
        rowVersion = findViewById(R.id.rowVersion)
        rowClearHistory = findViewById(R.id.rowClearHistory)
        rowLicenses = findViewById(R.id.rowLicenses)
        tvThemeValue = findViewById(R.id.tvThemeValue)
        tvColorModeValue = findViewById(R.id.tvColorModeValue)
        tvPinCodeValue = findViewById(R.id.tvPinCodeValue)
        tvDeviceNameValue = findViewById(R.id.tvDeviceNameValue)
        tvSaveLocationValue = findViewById(R.id.tvSaveLocationValue)
        tvVersionValue = findViewById(R.id.tvVersionValue)
        tvClearHistoryCount = findViewById(R.id.tvClearHistoryCount)
    }

    private fun setupNodeService() {
        SettingsManager.init(this)
        val intent = Intent(this, NodeService::class.java).apply {
            action = NodeService.ACTION_START
        }
        startService(intent)
        tvReceiveDeviceName.text = SettingsManager.deviceName
    }

    private fun setupPeerList() {
        deviceAdapter = DeviceListTileAdapter(
            onDeviceClick = { peer ->
                hapticTick()
                selectedDeviceHost = peer.host
                if (selectedUris.isNotEmpty()) {
                    selectedUris.forEach { uri ->
                        val si = Intent(this, NodeService::class.java).apply {
                            action = NodeService.ACTION_SEND_TO_MAC
                            putExtra("FILE_URI", uri)
                            putExtra("MAC_IP", peer.host)
                            putExtra("USE_ENCRYPTION", SettingsManager.encryptionEnabled)
                        }
                        startService(si)
                    }
                    selectedUris.clear()
                    fileSizeCache.clear()
                    saveQueueState()
                    updateQueueUI()
                } else if (pendingText != null) {
                    val si = Intent(this, NodeService::class.java).apply {
                        action = NodeService.ACTION_SEND_TEXT
                        putExtra("TEXT_CONTENT", pendingText)
                        putExtra("MAC_IP", peer.host)
                        putExtra("USE_ENCRYPTION", SettingsManager.encryptionEnabled)
                    }
                    startService(si)
                    pendingText = null
                    Toast.makeText(this, "Text sent", Toast.LENGTH_SHORT).show()
                } else {
                    Toast.makeText(this, "Select files or text to send", Toast.LENGTH_SHORT).show()
                }
            },
            onDeviceLongClick = { peer ->
                val isTrusted = SettingsManager.isTrustedPeer(peer.host)
                if (isTrusted) {
                    SettingsManager.removeTrustedPeer(peer.host)
                    Toast.makeText(this, "Removed ${peer.displayName} from trusted", Toast.LENGTH_SHORT).show()
                } else {
                    SettingsManager.addTrustedPeer(peer.host)
                    Toast.makeText(this, "${peer.displayName} added as trusted", Toast.LENGTH_SHORT).show()
                }
            }
        )
        rvDiscoveredDevices.layoutManager = LinearLayoutManager(this)
        rvDiscoveredDevices.adapter = deviceAdapter

        transferAdapter = TransferListAdapter(
            onCancel = { id -> FileTransferManager.cancelTransfer(id) },
            onRetry = { id ->
                val record = FileTransferManager.getHistory().find { it.id == id }
                if (record != null) FileTransferManager.resendTransfer(record)
            }
        )
        rvActiveTransfers.layoutManager = LinearLayoutManager(this)
        rvActiveTransfers.adapter = transferAdapter
        rvActiveTransfers.isNestedScrollingEnabled = false

        historyAdapter = TransferHistoryAdapter { record ->
            FileTransferManager.resendTransfer(record)
            Toast.makeText(this, "Resending ${record.filename}", Toast.LENGTH_SHORT).show()
        }
        rvTransferHistory.layoutManager = LinearLayoutManager(this)
        rvTransferHistory.adapter = historyAdapter
        rvTransferHistory.isNestedScrollingEnabled = false
    }

    private fun setupClickObservers() {
        bottomNavigation.setOnItemSelectedListener { item ->
            when (item.itemId) {
                R.id.menu_transfer -> { switchTab(R.id.menu_transfer); true }
                R.id.menu_devices -> { switchTab(R.id.menu_devices); true }
                R.id.menu_settings -> { switchTab(R.id.menu_settings); true }
                else -> false
            }
        }

        // Big buttons
        btnPhotos.setOnClickListener { pickPhotos() }
        btnFiles.setOnClickListener { pickFiles() }
        btnText.setOnClickListener { showTextInputDialog() }

        // File queue
        btnClearFiles.setOnClickListener {
            selectedUris.clear()
            fileSizeCache.clear()
            pendingText = null
            saveQueueState()
            updateQueueUI()
        }
        btnEditFiles.setOnClickListener { showFileListDialog() }
        btnAddFiles.setOnClickListener { pickFiles() }

        // Scanning
        btnRefreshScan.setOnClickListener {
            hapticTick()
            discoveryManager.clear()
            deviceAdapter.submitList(emptyList())
            updateDeviceListVisibility(false)
            val intent = Intent(this, NodeService::class.java).apply {
                action = NodeService.ACTION_START
            }
            startService(intent)
            Toast.makeText(this, "Refreshing...", Toast.LENGTH_SHORT).show()
        }
        btnManualIp.setOnClickListener { showConnectIpDialog() }

        // Troubleshoot
        btnTroubleshoot.setOnClickListener { showTroubleshootDialog() }

        // History clear (in receive tab)
        btnClearHistory.setOnClickListener { clearHistory() }

        // Settings switches
        switchAutoAccept.isChecked = SettingsManager.autoAccept
        switchAutoAccept.setOnCheckedChangeListener { _, isChecked -> SettingsManager.autoAccept = isChecked }

        switchEncryption.isChecked = SettingsManager.encryptionEnabled
        switchEncryption.setOnCheckedChangeListener { _, isChecked -> SettingsManager.encryptionEnabled = isChecked }

        switchZip.isChecked = SettingsManager.zipOnReceive
        switchZip.setOnCheckedChangeListener { _, isChecked -> SettingsManager.zipOnReceive = isChecked }

        // Settings rows
        rowTheme.setOnClickListener { showThemeDialog() }
        rowColorMode.setOnClickListener { showColorModeDialog() }
        rowPinCode.setOnClickListener { showPinCodeDialog() }
        rowDeviceName.setOnClickListener { showDeviceNameDialog() }
        rowSaveLocation.setOnClickListener { showSaveLocationDialog() }
        rowVersion.setOnClickListener {
            Toast.makeText(this, "Version ${tvVersionValue.text}", Toast.LENGTH_SHORT).show()
        }
        rowClearHistory.setOnClickListener { clearHistory() }
        rowLicenses.setOnClickListener { showLicensesDialog() }
    }

    // ═════════════════════════════════════════════════════════
    //  TAB SWITCHING
    // ═════════════════════════════════════════════════════════

    private fun switchTab(tabId: Int) {
        if (tabId == currentTab) return

        val show: View
        val hide: View
        val direction: Float

        when (tabId) {
            R.id.menu_transfer -> {
                show = layoutSend
                hide = if (currentTab == R.id.menu_devices) layoutReceive else layoutSettings
                direction = 1f
                layoutSettings.visibility = if (currentTab == R.id.menu_settings) View.GONE else layoutSettings.visibility
                layoutReceive.visibility = if (currentTab == R.id.menu_devices) View.GONE else layoutReceive.visibility
            }
            R.id.menu_devices -> {
                show = layoutReceive
                hide = if (currentTab == R.id.menu_transfer) layoutSend else layoutSettings
                direction = -1f
                layoutSend.visibility = if (currentTab == R.id.menu_transfer) View.GONE else layoutSend.visibility
                layoutSettings.visibility = if (currentTab == R.id.menu_settings) View.GONE else layoutSettings.visibility
            }
            else -> {
                show = layoutSettings
                hide = if (currentTab == R.id.menu_transfer) layoutSend else layoutReceive
                direction = 1f
                layoutSend.visibility = if (currentTab == R.id.menu_transfer) View.GONE else layoutSend.visibility
                layoutReceive.visibility = if (currentTab == R.id.menu_devices) View.GONE else layoutReceive.visibility
            }
        }

        show.alpha = 0f
        show.translationX = direction * 150f
        show.visibility = View.VISIBLE

        show.animate()
            .alpha(1f)
            .translationX(0f)
            .setDuration(300)
            .setInterpolator(FastOutSlowInInterpolator())
            .start()

        hide.animate()
            .alpha(0f)
            .translationX(-direction * 80f)
            .setDuration(250)
            .setInterpolator(FastOutSlowInInterpolator())
            .withEndAction { hide.visibility = View.GONE }
            .start()

        currentTab = tabId

        if (tabId == R.id.menu_transfer) {
            startScanningAnimation()
        } else {
            stopScanningAnimation()
        }

        if (tabId == R.id.menu_devices) {
            startReceiveLogoAnimation()
        } else {
            stopReceiveLogoAnimation()
        }
    }

    // ═════════════════════════════════════════════════════════
    //  HAPTIC FEEDBACK
    // ═════════════════════════════════════════════════════════

    private fun hapticTick() {
        (currentFocus ?: window.decorView).performHapticFeedback(
            if (Build.VERSION.SDK_INT >= 30) HapticFeedbackConstants.CONFIRM
            else HapticFeedbackConstants.KEYBOARD_TAP
        )
    }

    // ═════════════════════════════════════════════════════════
    //  SCANNING INDICATOR
    // ═════════════════════════════════════════════════════════

    private fun startScanningAnimation() {
        scanningDot.visibility = View.VISIBLE
        scanningDot.animate()
            .alpha(0.3f)
            .scaleX(1.5f)
            .scaleY(1.5f)
            .setDuration(800)
            .withEndAction {
                scanningDot.animate()
                    .alpha(1f)
                    .scaleX(1f)
                    .scaleY(1f)
                    .setDuration(800)
                    .withEndAction { startScanningAnimation() }
                    .start()
            }
            .start()
    }

    private fun stopScanningAnimation() {
        scanningDot.animate().cancel()
        scanningDot.visibility = View.GONE
    }

    // ═════════════════════════════════════════════════════════
    //  RECEIVE LOGO ANIMATION
    // ═════════════════════════════════════════════════════════

    private var receiveLogoAnim: RotateAnimation? = null

    private fun startReceiveLogoAnimation() {
        if (receiveLogoAnim != null) return
        val anim = RotateAnimation(
            0f, 360f,
            RotateAnimation.RELATIVE_TO_SELF, 0.5f,
            RotateAnimation.RELATIVE_TO_SELF, 0.5f
        ).apply {
            duration = 3000
            repeatCount = RotateAnimation.INFINITE
            interpolator = FastOutSlowInInterpolator()
        }
        receiveLogo.startAnimation(anim)
        receiveLogoAnim = anim
    }

    private fun stopReceiveLogoAnimation() {
        receiveLogoAnim?.cancel()
        receiveLogoAnim = null
        receiveLogo.clearAnimation()
    }

    // ═════════════════════════════════════════════════════════
    //  BIG BUTTON HANDLERS
    // ═════════════════════════════════════════════════════════

    private fun pickPhotos() {
        if (Build.VERSION.SDK_INT >= 33) {
            try {
                photoPickerLauncher.launch(null)
            } catch (_: Exception) {
                filePickerLauncher.launch("image/*")
            }
        } else {
            filePickerLauncher.launch("image/*,video/*")
        }
    }

    private fun pickFiles() {
        filePickerLauncher.launch("*/*")
    }

    private fun showTextInputDialog() {
        val input = EditText(this).apply {
            hint = "Enter text to send"
            setSingleLine()
        }
        MaterialAlertDialogBuilder(this)
            .setTitle("Send Text")
            .setView(input)
            .setPositiveButton("Send") { _, _ ->
                val text = input.text.toString().trim()
                if (text.isNotEmpty()) {
                    pendingText = text
                    Toast.makeText(this, "Text ready — tap a device to send", Toast.LENGTH_SHORT).show()
                }
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun pasteFromClipboard() {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = clipboard.primaryClip
        if (clip != null && clip.itemCount > 0) {
            val text = clip.getItemAt(0).text?.toString()?.trim()
            if (!text.isNullOrEmpty()) {
                pendingText = text
                Toast.makeText(this, "Text pasted from clipboard", Toast.LENGTH_SHORT).show()
                return
            }
        }
        Toast.makeText(this, "Clipboard is empty", Toast.LENGTH_SHORT).show()
    }

    private fun addFiles(uris: List<Uri>) {
        for (uri in uris) {
            val uriStr = uri.toString()
            if (selectedUris.none { it.toString() == uriStr }) {
                selectedUris.add(uri)
                if (!fileSizeCache.containsKey(uriStr)) {
                    fileSizeCache[uriStr] = resolveFileSize(uri)
                }
            }
        }
        saveQueueState()
        updateQueueUI()
    }

    // ═════════════════════════════════════════════════════════
    //  FILE QUEUE UI
    // ═════════════════════════════════════════════════════════

    private fun updateQueueUI() {
        if (selectedUris.isEmpty()) {
            cardFileQueue.visibility = View.GONE
        } else {
            cardFileQueue.visibility = View.VISIBLE
            tvFileCount.text = "${selectedUris.size} file${if (selectedUris.size > 1) "s" else ""}"
            val totalSize = selectedUris.sumOf { fileSizeCache[it.toString()] ?: 0L }
            tvFileTotalSize.text = FileTransferManager.formatSize(totalSize)

            thumbnailContainer.removeAllViews()
            val density = resources.displayMetrics.density
            for (uri in selectedUris.take(20)) {
                val name = getFileName(uri).lowercase()
                val isImage = name.endsWith(".jpg") || name.endsWith(".jpeg") ||
                    name.endsWith(".png") || name.endsWith(".gif") || name.endsWith(".webp")
                val iv = ImageView(this).apply {
                    layoutParams = LinearLayout.LayoutParams(
                        (48 * density).toInt(), (48 * density).toInt()
                    ).apply { marginEnd = (8 * density).toInt() }
                    scaleType = if (isImage) ImageView.ScaleType.CENTER_CROP
                        else ImageView.ScaleType.CENTER_INSIDE
                    if (isImage) {
                        try { setImageURI(uri) } catch (_: Exception) {
                            setImageResource(R.drawable.ic_photos)
                        }
                    } else {
                        setPadding(
                            (12 * density).toInt(), (12 * density).toInt(),
                            (12 * density).toInt(), (12 * density).toInt()
                        )
                        setImageResource(R.drawable.ic_file_generic)
                    }
                }
                thumbnailContainer.addView(iv)
            }
        }
    }

    private fun showFileListDialog() {
        val items = selectedUris.map { uri ->
            val name = getFileName(uri)
            val size = fileSizeCache[uri.toString()] ?: 0L
            "$name (${FileTransferManager.formatSize(size)})"
        }.toTypedArray()
        MaterialAlertDialogBuilder(this)
            .setTitle("Selected Files")
            .setItems(items) { _, _ -> }
            .setPositiveButton("OK", null)
            .show()
    }

    // ═════════════════════════════════════════════════════════
    //  DEVICE LIST UI
    // ═════════════════════════════════════════════════════════

    private fun updateDeviceListVisibility(hasDevices: Boolean) {
        rvDiscoveredDevices.visibility = if (hasDevices) View.VISIBLE else View.GONE
        tvNoDevices.visibility = if (hasDevices) View.GONE else View.VISIBLE
    }

    // ═════════════════════════════════════════════════════════
    //  TRANSFERS UI
    // ═════════════════════════════════════════════════════════

    private fun updateTransfersPanelVisibility(hasTransfers: Boolean) {
        if (hasTransfers && activeTransfersPanel.visibility != View.VISIBLE) {
            activeTransfersPanel.visibility = View.VISIBLE
            activeTransfersPanel.alpha = 0f
            activeTransfersPanel.animate().alpha(1f).setDuration(300).start()
        } else if (!hasTransfers && activeTransfersPanel.visibility == View.VISIBLE) {
            activeTransfersPanel.animate().alpha(0f).setDuration(300)
                .withEndAction { activeTransfersPanel.visibility = View.GONE }.start()
        }
    }

    // ═════════════════════════════════════════════════════════
    //  HISTORY
    // ═════════════════════════════════════════════════════════

    private fun updateHistory() {
        val history = FileTransferManager.getHistory()
        historyAdapter.submitList(history)
        historyPanel.visibility = if (history.isEmpty()) View.GONE else View.VISIBLE
    }

    private fun clearHistory() {
        MaterialAlertDialogBuilder(this)
            .setTitle("Clear History")
            .setMessage("Delete all transfer history?")
            .setPositiveButton("Clear") { _, _ ->
                FileTransferManager.clearHistory()
                updateHistory()
                updateSettingsValues()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    // ═════════════════════════════════════════════════════════
    //  SETTINGS
    // ═════════════════════════════════════════════════════════

    private fun updateSettingsValues() {
        tvDeviceNameValue.text = SettingsManager.deviceName
        tvVersionValue.text = getVersionName()
        val historyCount = FileTransferManager.getHistory().size
        tvClearHistoryCount.text = "$historyCount items"
        val pin = settingsPrefs.getString("pin_code", "")
        tvPinCodeValue.text = if (pin.isNullOrEmpty()) "None" else "****"
        val saveLoc = SettingsManager.getDownloadDir().absolutePath
        tvSaveLocationValue.text = saveLoc.substringAfterLast('/')
        val theme = settingsPrefs.getString("theme_mode", "system")
        tvThemeValue.text = (theme ?: "system").replaceFirstChar(Char::uppercaseChar)
        val color = settingsPrefs.getString("color_mode", "system")
        tvColorModeValue.text = when (color) {
            "localsend" -> "LocalSend"
            "oled" -> "OLED"
            else -> "System"
        }
    }

    private fun getVersionName(): String {
        return try {
            packageManager.getPackageInfo(packageName, 0).versionName ?: "1.0"
        } catch (_: Exception) { "1.0" }
    }

    private fun showThemeDialog() {
        val options = arrayOf("System", "Dark", "Light")
        val current = when (settingsPrefs.getString("theme_mode", "system")) {
            "dark" -> 1
            "light" -> 2
            else -> 0
        }
        MaterialAlertDialogBuilder(this)
            .setTitle("Theme")
            .setSingleChoiceItems(options, current) { dialog, which ->
                val value = when (which) { 1 -> "dark"; 2 -> "light"; else -> "system" }
                settingsPrefs.edit().putString("theme_mode", value).apply()
                tvThemeValue.text = options[which]
                dialog.dismiss()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun showColorModeDialog() {
        val options = arrayOf("System", "LocalSend", "OLED")
        val current = when (settingsPrefs.getString("color_mode", "system")) {
            "localsend" -> 1
            "oled" -> 2
            else -> 0
        }
        MaterialAlertDialogBuilder(this)
            .setTitle("Color Mode")
            .setSingleChoiceItems(options, current) { dialog, which ->
                val value = when (which) { 1 -> "localsend"; 2 -> "oled"; else -> "system" }
                settingsPrefs.edit().putString("color_mode", value).apply()
                tvColorModeValue.text = options[which]
                dialog.dismiss()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun showPinCodeDialog() {
        val input = EditText(this).apply {
            hint = "Enter PIN code"
            inputType = android.text.InputType.TYPE_CLASS_NUMBER
            setSingleLine()
        }
        MaterialAlertDialogBuilder(this)
            .setTitle("PIN Code")
            .setMessage("Set a PIN for incoming transfers")
            .setView(input)
            .setPositiveButton("Save") { _, _ ->
                val pin = input.text.toString().trim()
                settingsPrefs.edit().putString("pin_code", pin).apply()
                tvPinCodeValue.text = if (pin.isEmpty()) "None" else "****"
            }
            .setNegativeButton("Remove") { _, _ ->
                settingsPrefs.edit().putString("pin_code", "").apply()
                tvPinCodeValue.text = "None"
            }
            .setNeutralButton("Cancel", null)
            .show()
    }

    private fun showDeviceNameDialog() {
        val input = EditText(this).apply {
            setText(SettingsManager.deviceName)
            selectAll()
        }
        MaterialAlertDialogBuilder(this)
            .setTitle("Device Name")
            .setView(input)
            .setPositiveButton("Save") { _, _ ->
                val name = input.text.toString().trim()
                if (name.isNotEmpty()) {
                    SettingsManager.deviceName = name
                    tvReceiveDeviceName.text = name
                    tvDeviceNameValue.text = name
                }
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun showSaveLocationDialog() {
        val currentPath = SettingsManager.getDownloadDir().absolutePath
        MaterialAlertDialogBuilder(this)
            .setTitle("Save Location")
            .setMessage("Current: $currentPath\n\nUse system file picker to change.")
            .setPositiveButton("Change") { _, _ ->
                Toast.makeText(this, "Use system file manager to choose a folder", Toast.LENGTH_LONG).show()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun showLicensesDialog() {
        MaterialAlertDialogBuilder(this)
            .setTitle("Open Source Licenses")
            .setMessage(
                "This application uses the following open source libraries:\n\n" +
                "- Kotlin Coroutines (Apache 2.0)\n" +
                "- Material Components (Apache 2.0)\n" +
                "- AndroidX (Apache 2.0)\n" +
                "- NIO Transfer (MIT)\n\n" +
                "Copyright (c) 2024 DOS Agent"
            )
            .setPositiveButton("OK", null)
            .show()
    }

    private fun showTroubleshootDialog() {
        MaterialAlertDialogBuilder(this)
            .setTitle("Troubleshooting")
            .setMessage(
                "Make sure:\n\n" +
                "1. Both devices are on the same Wi-Fi network\n" +
                "2. Wi-Fi is enabled on both devices\n" +
                "3. No firewall is blocking connections\n" +
                "4. Both devices have the app open\n\n" +
                "Local IP: ${getLocalIpAddress()}\n" +
                "Device: ${SettingsManager.deviceName}"
            )
            .setPositiveButton("OK", null)
            .show()
    }

    // ═════════════════════════════════════════════════════════
    //  HELPERS
    // ═════════════════════════════════════════════════════════

    private fun updateLocalNetworkInfo() {
        tvReceiveIp.text = "IP: ${getLocalIpAddress()}"
    }

    private fun getLocalIpAddress(): String {
        try {
            val interfaces = java.util.Collections.list(java.net.NetworkInterface.getNetworkInterfaces())
            for (intf in interfaces) {
                val addrs = java.util.Collections.list(intf.inetAddresses)
                for (addr in addrs) {
                    if (!addr.isLoopbackAddress) {
                        val sAddr = addr.hostAddress ?: continue
                        if (sAddr.indexOf(':') < 0) return sAddr
                    }
                }
            }
        } catch (_: Exception) {}
        return "Offline"
    }

    private fun resolveFileSize(uri: Uri): Long {
        return try {
            contentResolver.openFileDescriptor(uri, "r")?.statSize ?: 0L
        } catch (_: Exception) { 0L }
    }

    private fun getFileName(uri: Uri): String {
        var name = ""
        if (uri.scheme == "content") {
            try {
                contentResolver.query(uri, null, null, null, null)?.use { cursor ->
                    if (cursor.moveToFirst()) {
                        val idx = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                        if (idx >= 0) name = cursor.getString(idx) ?: ""
                    }
                }
            } catch (_: Exception) {}
        }
        if (name.isEmpty()) name = uri.path?.substringAfterLast('/') ?: "file"
        return name
    }

    private fun saveQueueState() {
        getSharedPreferences("pdos_queue", Context.MODE_PRIVATE).edit()
            .putStringSet("queued_uris", selectedUris.map { it.toString() }.toSet()).apply()
    }

    private fun loadQueueState() {
        val prefs = getSharedPreferences("pdos_queue", Context.MODE_PRIVATE)
        val uriStrings = prefs.getStringSet("queued_uris", null) ?: return
        selectedUris.clear()
        fileSizeCache.clear()
        for (str in uriStrings) {
            try {
                val uri = Uri.parse(str)
                selectedUris.add(uri)
                fileSizeCache[str] = resolveFileSize(uri)
            } catch (_: Exception) {}
        }
    }

    private fun handleIntent(intent: Intent) {
        if (intent.action == Intent.ACTION_SEND && intent.type != null) {
            val uri = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                @Suppress("DEPRECATION") intent.getParcelableExtra(Intent.EXTRA_STREAM)
            }
            if (uri != null) addFiles(listOf(uri))
            bottomNavigation.selectedItemId = R.id.menu_transfer
        }
    }

    private fun showConnectIpDialog() {
        val input = EditText(this).apply {
            hint = "192.168.1.15"
            inputType = android.text.InputType.TYPE_CLASS_TEXT
            setSingleLine()
        }
        MaterialAlertDialogBuilder(this)
            .setTitle("Connect by IP")
            .setView(input)
            .setPositiveButton("Connect") { _, _ ->
                val ip = input.text.toString().trim()
                if (ip.isNotEmpty()) {
                    val useEncryption = SettingsManager.encryptionEnabled
                    val port = if (useEncryption) 8443 else 8080
                    lifecycleScope.launch(Dispatchers.IO) {
                        val caps = try { NioTransfer.performHandshake(ip, port) } catch (_: Exception) { null }
                        withContext(Dispatchers.Main) {
                            if (caps != null) {
                                discoveryManager.addOrUpdate(
                                    name = caps.node_id, host = ip, port = port, platform = "mac"
                                )
                                Toast.makeText(this@MainActivity, "Connected to ${caps.node_id}!", Toast.LENGTH_SHORT).show()
                            } else {
                                Toast.makeText(this@MainActivity, "Connection failed", Toast.LENGTH_LONG).show()
                            }
                        }
                    }
                }
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun updateFabVisibility() {
        // No FAB in LocalSend layout
    }
}

internal object DynamicColorsCompat {
    fun applyToActivity(activity: AppCompatActivity) {
        try {
            val cls = Class.forName("com.google.android.material.color.DynamicColors")
            val method = cls.getMethod("applyToActivityIfAvailable", AppCompatActivity::class.java)
            method.invoke(null, activity)
        } catch (_: Exception) {}
    }
}
