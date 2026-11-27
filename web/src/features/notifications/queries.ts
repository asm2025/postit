import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import type { components } from '@/api/schema'

export type NotificationDto = components['schemas']['NotificationDto']

const POLL_MS = 60_000

/**
 * How many notifications are unread, for the bell. Polled while the tab is visible and again
 * when it regains focus; the server answers 304 to an unchanged count, which the browser's cache
 * turns back into the count.
 */
export function useUnreadCount() {
  const api = useApi()
  return useQuery({
    queryKey: ['notifications', 'unread'],
    queryFn: async () => (await unwrap(api.GET('/api/v1/notifications/unread-count'))).count,
    refetchInterval: POLL_MS,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
  })
}

/** The caller's notifications, newest first. The route pages only: read state shows on each row. */
export function useNotifications(page: number, pageSize: number) {
  const api = useApi()
  return useQuery({
    queryKey: ['notifications', 'list', page, pageSize],
    queryFn: () => unwrap(api.GET('/api/v1/notifications', { params: { query: { page, page_size: pageSize } } })),
    placeholderData: keepPreviousData,
  })
}

/** One notification, which a Web Push click names by ID. A 404 is a notification that is gone: no retry. */
export function useNotification(id: string | null) {
  const api = useApi()
  return useQuery({
    queryKey: ['notifications', 'one', id],
    queryFn: () => unwrap(api.GET('/api/v1/notifications/{id}', { params: { path: { id: id ?? '' } } })),
    enabled: id !== null,
    retry: false,
  })
}

export function useMarkRead() {
  const api = useApi()
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (selection: { ids: string[] } | { all: true }) =>
      unwrap(api.POST('/api/v1/notifications/read', { body: selection })),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['notifications'] }),
  })
}
