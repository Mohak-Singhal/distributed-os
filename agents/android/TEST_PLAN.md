# PDOS Android — Final Test Plan

## Prerequisites
- Android device (API 30+) with the APK installed
- Mac with `dos dashboard 8080` running
- Both devices on the same WiFi network
- At least 500MB free storage

## 1. Discovery

| Test | Steps | Expected |
|------|-------|----------|
| Mac appears in peer list | Tap SEARCH button | Mac shown in Send tab within 10s |
| No WiFi shows warning | Turn off WiFi, open app | "Not on WiFi" message shown |
| Peer lost on disconnect | Disconnect Mac from network | Peer disappears from list within 30s |
| Multiple Macs discovered | Run dashboard on 2+ Macs | All shown in peer list |
| Re-discovery works | Stop then start search again | Peers re-appear |

## 2. File Transfer — Android → Mac

| Test | Steps | Expected |
|------|-------|----------|
| Small file (1KB text) | Share → PDOS → select peer | File arrives in ~/Downloads/PDOS/ |
| Large file (100MB video) | Same flow | Progress bar, speed, ETA shown |
| Multiple files (5 photos) | Select 5 in picker | Each has separate progress line |
| Text sharing | Type in Send tab → Send | clipboard_<timestamp>.txt arrives |
| UTF-8 filename (中文.jpg) | Share file with non-ASCII name | Filename preserved on Mac |
| Cancel mid-transfer | Tap ✕ on active transfer | File deleted, history shows Cancelled |
| Encryption ON | Enable TLS toggle, send file | Transfer uses HTTPS (port 8443) |

## 3. File Transfer — Mac → Android

| Test | Steps | Expected |
|------|-------|----------|
| Receive small file | Send from Mac dashboard | Accept/Decline notification appears |
| Accept transfer | Tap Accept | File downloads, notification updates |
| Decline transfer | Tap Decline | Nothing downloaded |
| Auto-accept trusted | Long-press peer → Trust → auto-accept ON | Files arrive without prompt |
| Pairing code shown | Untrusted peer sends file | 4-digit code in notification |
| Resume after disconnect | Kill app mid-download, reopen | Download resumes from last byte |
| ZIP on receive | Enable in settings, receive file | Received as .zip, then extracted |

## 4. Transfer History

| Test | Steps | Expected |
|------|-------|----------|
| History persists | Send file, kill app, reopen | Transfer shown in Monitor tab |
| Resend from history | Tap ↻ Resend | Opens file picker, sends to same peer |
| Thumbnail shown | Send/receive image | Thumbnail visible in history row |
| Speed display | Check Monitor tab after transfer | Avg speed correctly calculated |

## 5. Simultaneous Transfers

| Test | Steps | Expected |
|------|-------|----------|
| 3 uploads at once | Select 3 files, send | 3 progress bars shown in Send tab |
| Upload + download | Send one, receive another | Both tabs show active transfers |
| Cancel one of many | Cancel 1 of 3 uploads | Other 2 continue unaffected |

## 6. Speed Limiting

| Test | Steps | Expected |
|------|-------|----------|
| Unlimited | Default setting | Full bandwidth used |
| 1 MB/s limit | Set in Settings | Transfer completes slower |
| 128 KB/s limit | Transfer large file | Visible bandwidth cap |

## 7. Edge Cases

| Test | Steps | Expected |
|------|-------|----------|
| Storage full | Fill device storage, send file | Error: "Not enough storage" |
| Permission denied | Remove file permission, re-share | Error: "Permission denied" |
| Battery optimization | Leave app, start transfer | Transfer completes in background |
| Very large file (4GB) | Share 4GB file | Transfer completes (may take long) |
| Screen off | Start transfer, turn off screen | Transfer continues (foreground service) |
| IPv6-only network | Connect to IPv6 hotspot | Discovery + transfer works |

## 8. Regression

| Test | Steps | Expected |
|------|-------|----------|
| Screen mirror still works | Tap START MIRROR | Mirror streams on port 7892 |
| Camera still streams | Tap START CAMERA | Camera streams on port 7893 |
| Search still works | Connection lost → SEARCH again | Reconnects without crash |
