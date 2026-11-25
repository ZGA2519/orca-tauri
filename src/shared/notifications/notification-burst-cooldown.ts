import type { NotificationDispatchRequest } from './notification-settings-types'

export const NOTIFICATION_COOLDOWN_MS = 5000

// Keyed news is announced once by its producer, so only its own repeat may collapse it.
export const notificationDedupeKey = (request: NotificationDispatchRequest): string =>
  JSON.stringify([
    request.executionHost,
    request.attentionKey ?? request.worktreeId ?? request.worktreeLabel ?? 'global'
  ])
const MAX_RECENT_NOTIFICATION_KEYS = 50

function pruneRecentNotifications(recentNotifications: Map<string, number>, now: number): void {
  if (recentNotifications.size <= MAX_RECENT_NOTIFICATION_KEYS) {
    return
  }

  for (const [key, ts] of recentNotifications) {
    if (now - ts >= NOTIFICATION_COOLDOWN_MS) {
      recentNotifications.delete(key)
    }
  }

  while (recentNotifications.size > MAX_RECENT_NOTIFICATION_KEYS) {
    const oldest = recentNotifications.keys().next()
    if (oldest.done) {
      break
    }
    recentNotifications.delete(oldest.value)
  }
}

export function reserveNotificationCooldown(
  recentNotifications: Map<string, number>,
  dedupeKey: string,
  now: number
): boolean {
  const lastSentAt = recentNotifications.get(dedupeKey) ?? 0
  if (now - lastSentAt < NOTIFICATION_COOLDOWN_MS) {
    return false
  }
  recentNotifications.delete(dedupeKey)
  recentNotifications.set(dedupeKey, now)
  pruneRecentNotifications(recentNotifications, now)
  return true
}
