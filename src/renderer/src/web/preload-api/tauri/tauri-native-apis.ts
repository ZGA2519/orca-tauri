import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import { openPath, openUrl, revealItemInDir } from '@tauri-apps/plugin-opener'
import type { PreloadApi } from '../../../../../preload/api-types'
import type { ExecutionHostId } from '../../../../../shared/execution-host/execution-host'
import { LOCAL_EXECUTION_HOST_ID } from '../../../../../shared/execution-host/execution-host'
import { TRASH_DELETE_FAILED_MESSAGE } from '../../../../../shared/file-access/trash-delete-failure'
import { createTauriNotificationsApi } from './tauri-notifications-api'

type NativeOverridableApi = Partial<
  Pick<PreloadApi, 'ui' | 'shell' | 'repos' | 'notifications' | 'fs'>
> & {
  settings?: Partial<Pick<PreloadApi['settings'], 'get'>>
}

// orcad runs on this machine, so its 'local' host paths are paths the shell can open.
const isShellLocal = (hostId: ExecutionHostId): boolean => hostId === LOCAL_EXECUTION_HOST_ID

async function pickDirectory(defaultPath?: string): Promise<string | null> {
  const picked = await open({ directory: true, defaultPath })
  return typeof picked === 'string' ? picked : null
}

function onMaximizeChanged(callback: (isMaximized: boolean) => void): () => void {
  const appWindow = getCurrentWindow()
  let last: boolean | null = null
  let disposed = false
  const unlisten = appWindow.onResized(async () => {
    const next = await appWindow.isMaximized()
    if (!disposed && next !== last) {
      last = next
      callback(next)
    }
  })
  return () => {
    disposed = true
    void unlisten.then((stop) => stop())
  }
}

/** Swaps the web client's no-op window, shell and picker stubs for the Tauri shell's native ones. */
export function withTauriNativeApis<T extends NativeOverridableApi>(api: T): T {
  const { ui, shell, repos, notifications, fs } = api
  const readSettings = api.settings?.get
  if (!ui || !shell || !repos || !notifications || !fs || !readSettings) {
    return api
  }
  const appWindow = getCurrentWindow()
  let zoomLevel = ui.getZoomLevel()
  const { openPath: webOpenPath, openInFileManager: webOpenInFileManager } = shell
  return {
    ...api,
    ui: {
      ...ui,
      minimize: () => void appWindow.minimize(),
      maximize: () => void appWindow.toggleMaximize(),
      isMaximized: () => appWindow.isMaximized(),
      onMaximizeChanged,
      getZoomLevel: () => zoomLevel,
      setZoomLevel: (level) => {
        zoomLevel = level
        ui.setZoomLevel(level)
        // Electron zoom levels scale by 1.2 per step.
        void getCurrentWebview().setZoom(1.2 ** level)
      }
    },
    shell: {
      ...shell,
      openUrl: (url) => openUrl(url),
      openPath: (path, hostId) =>
        isShellLocal(hostId) ? openPath(path) : webOpenPath(path, hostId),
      openInFileManager: async (path, hostId) => {
        if (!isShellLocal(hostId)) {
          return webOpenInFileManager(path, hostId)
        }
        try {
          await revealItemInDir(path)
          return { ok: true }
        } catch {
          return { ok: false, reason: 'launch-failed' }
        }
      },
      pickDirectory: (args) => pickDirectory(args?.defaultPath)
    },
    repos: { ...repos, pickDirectory: () => pickDirectory() },
    notifications: createTauriNotificationsApi(notifications, readSettings),
    fs: {
      ...fs,
      // orcad can only delete permanently; local deletes go to the Trash like Electron's.
      deletePath: async (args) => {
        if (args.permanent || args.connectionId) {
          return fs.deletePath(args)
        }
        try {
          await invoke('move_to_trash', { path: args.targetPath })
        } catch (error) {
          // The file explorer offers a permanent retry only for this message.
          throw new Error(`${TRASH_DELETE_FAILED_MESSAGE}: ${String(error)}`)
        }
      }
    }
  }
}
