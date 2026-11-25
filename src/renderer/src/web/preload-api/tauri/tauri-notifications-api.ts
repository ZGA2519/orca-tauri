import {
  isPermissionGranted,
  requestPermission,
  sendNotification
} from '@tauri-apps/plugin-notification'
import { translate } from '@/i18n/i18n'
import type { PreloadApi } from '../../../../../preload/api-types'
import type { GlobalSettings } from '../../../../../shared/settings/global-settings-types'
import { buildNotificationOptions } from '../../../../../shared/notifications/notification-options'
import { isNotificationSourceEnabled } from '../../../../../shared/notifications/notification-source-gate'
import {
  notificationDedupeKey,
  reserveNotificationCooldown
} from '../../../../../shared/notifications/notification-burst-cooldown'
import { getBrowserPlatform } from '../web-storage'

type NotificationsApi = NonNullable<Partial<PreloadApi>['notifications']>

/**
 * Native desktop notifications with the Electron delivery gates (source, mute, focus, cooldown).
 * ponytail: no click-to-focus (the plugin has no desktop actions) and no phone fan-out; orcad pushes to phones itself.
 */
export function createTauriNotificationsApi(
  web: NotificationsApi,
  readSettings: () => Promise<GlobalSettings>
): NotificationsApi {
  const recent = new Map<string, number>()
  let requested = false
  const hasPermission = async (): Promise<boolean> => {
    if (await isPermissionGranted()) {
      return true
    }
    requested = true
    return (await requestPermission()) === 'granted'
  }

  return {
    ...web,
    dispatch: async (request) => {
      const settings = (await readSettings()).notifications
      const hostMuted =
        request.notificationSourceId !== undefined &&
        settings.mutedNotificationSourceIds.includes(request.notificationSourceId)
      if (!isNotificationSourceEnabled(settings, request.source, request.agentState) || hostMuted) {
        return {
          delivered: false,
          reason: !settings.enabled ? 'disabled' : hostMuted ? 'host-muted' : 'source-disabled'
        }
      }
      if (settings.suppressWhenFocused && request.isActiveWorktree && document.hasFocus()) {
        return { delivered: false, reason: 'suppressed-focus' }
      }
      if (
        request.source !== 'test' &&
        !reserveNotificationCooldown(recent, notificationDedupeKey(request), Date.now())
      ) {
        return { delivered: false, reason: 'cooldown' }
      }
      if (!(await hasPermission())) {
        return { delivered: false, reason: 'blocked-by-system' }
      }
      const { title, body } = buildNotificationOptions(request, translate)
      sendNotification({ title, body })
      return { delivered: true }
    },
    getPermissionStatus: async () => ({
      supported: true,
      platform: getBrowserPlatform(),
      requested
    })
  }
}
