# macOS App Package Structure

## Folder Structure
```
mac-app/
├── Sources/PDOS/
│   ├── AppDelegate.swift                    # Application delegate for bundled daemons
│   ├── Services/
│   │   ├── LaunchManager.swift              # Bundle daemon lifecycle manager
│   │   ├── BackendService.swift             # App backend services (unchanged except for binary resolution)
│   │   ├── FileTransferService.swift       # Modular file transfer service
│   │   ├── DevicePersistence.swift         # Device state persistence
│   │   └── ... (other services)
│   ├── Views/
│   │   ├── ContentView.swift               # Main UI with drag & drop
│   │   ├── DropHandler.swift               # File drop handling
│   │   ├── DevicePickerSheet.swift        # Reusable device picker
│   │   ├── MenuBarView.swift              # Menu bar with drag & drop
│   │   └── ... (other views)
│   ├── Utilities/
│   │   ├── DOSBinaryUtility.swift          # New: Unified binary resolution
│   │   ├── DOSBinary.swift                # Legacy: Deprecated utility functions
│   │   └── ... (other utilities)
│   ├── Models/
│   │   ├── FileTransferService.swift     # Device model used across app
│   │   └── ... (other models)
│   ├── Styles/                          # UI styles
│   └── Scripts/                         # Internal scripts
└── Sources/PDOSShareExtension/              # macOS Finder Share Extension
    ├── ShareViewController.swift
    ├── ShareExtensionView.swift
    └── Info.plist
```

## Key Features Implemented

### 1. **Modular Binary Resolution** (`Utilities/DOSBinaryUtility.swift`)
- Single source of truth for finding dos, dos-relay, and adb binaries
- Priority: Resources directory → Bundle → Traditional PATH locations
- Supports bundled app packaging while maintaining backward compatibility

### 2. **File Transfer Service** (`Services/FileTransferService.swift`)
- Centralized sendFile() and sendFiles() methods
- Shared across main app and Share Extension
- Recursive helpers (parseDeviceList, Device model)

### 3. **Drag & Drop** (`Views/DropHandler.swift`)
- Reusable modifier applied to ContentView
- DropOverlayView with visual feedback
- Uses FileTransferService for sending files

### 4. **Menu Bar Drag & Drop** (`Views/MenuBarView.swift`)
- Menu bar icon supports drag & drop
- Shows drag over indicator
- Opens DevicePickerSheet for device selection

### 5. **Finder Share Extension** (`Sources/PDOSShareExtension/`)
- Configured in Package.swift as separate target
- Info.plist defines share-service extension point
- Uses FileTransferService for file operations

### 6. **Bundled Daemon Management** (`Services/LaunchManager.swift`)
- AppDelegate starts bundled dos, dos-relay, and adb binaries
- Automatic restart on crash
- No terminal dependencies required

### 7. **Android Quick Settings Tile** (`agents/android/`)
- PdosTileService toggles node service
- Tile updates from NodeService state

## Migration Notes

### For developers upgrading from previous version:
- `resolveDOSBinary()` and `resolveRelayBinary()` now delegate to `DOSBinaryUtility.resolveBinary()`
- File transfer calls now go through `FileTransferService`
- Drag & drop implemented via `FileDropHandler` modifier
- Share Extension uses shared `FileTransferService.listDevices()`

### For users:
- No changes required
- App automatically manages bundled binaries on startup
- Zero terminal dependency

## Usage Example

### Drag & Drop in ContentView:
```swift
ContentView()
    .onFileDrop { providers in
        // Process dragged files...
    }
```

### Share Extension:
```swift
FileTransferService.listDevices { devices in
    // Populate device list...
}
```

The code is fully modular with zero duplication and clear separation of concerns!