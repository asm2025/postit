import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { ensureOk, unwrap } from '@/api/client'
import type { components } from '@/api/schema'

export type User = components['schemas']['UserDto']
export type UserStatus = User['status']
export type UserRole = User['role']
export type UserSort = 'created_at' | '-created_at'

export interface UserFilters {
  status?: UserStatus
  role?: UserRole
  search?: string
  sort?: UserSort
  page: number
  pageSize?: number
}

function query(f: UserFilters) {
  return {
    status: f.status,
    role: f.role,
    search: f.search ?? undefined,
    sort: f.sort,
    page: f.page,
    page_size: f.pageSize ?? 20,
  }
}

export function useUsers(filters: UserFilters, opts: { refetchInterval?: number } = {}) {
  const api = useApi()
  return useQuery({
    queryKey: ['users', 'list', filters],
    queryFn: () => unwrap(api.GET('/api/v1/users', { params: { query: query(filters) } })),
    placeholderData: keepPreviousData,
    refetchInterval: opts.refetchInterval,
  })
}

/** Total matching users via a one-row page; the `enabled` flag keeps members off admin routes. */
export function useUserCount(filters: Omit<UserFilters, 'page'>, enabled = true) {
  const api = useApi()
  return useQuery({
    queryKey: ['users', 'count', filters],
    queryFn: async () =>
      (await unwrap(api.GET('/api/v1/users', { params: { query: query({ ...filters, page: 1, pageSize: 1 }) } })))
        .total,
    enabled,
    refetchInterval: 30_000,
  })
}

/** Count of users waiting for approval (the sidebar badge and Home panel). */
export function usePendingCount(enabled: boolean) {
  return useUserCount({ status: 'pending' }, enabled)
}

export function useUserActions() {
  const api = useApi()
  const qc = useQueryClient()
  const done = () => {
    void qc.invalidateQueries({ queryKey: ['users'] })
    void qc.invalidateQueries({ queryKey: ['audit'] })
    // Cheap, and the only way an admin's change to their own row reaches the gate.
    void qc.invalidateQueries({ queryKey: ['me'] })
  }
  const patch = (body: { status?: UserStatus; role?: UserRole }) => (id: string) =>
    ensureOk(api.PATCH('/api/v1/users/{id}', { params: { path: { id } }, body }))
  return {
    approve: useMutation({ mutationFn: patch({ status: 'active' }), onSuccess: done }),
    disable: useMutation({ mutationFn: patch({ status: 'disabled' }), onSuccess: done }),
    enable: useMutation({ mutationFn: patch({ status: 'active' }), onSuccess: done }),
    setRole: useMutation({
      mutationFn: ({ id, role }: { id: string; role: UserRole }) => patch({ role })(id),
      onSuccess: done,
    }),
    remove: useMutation({
      mutationFn: (id: string) => ensureOk(api.DELETE('/api/v1/users/{id}', { params: { path: { id } } })),
      onSuccess: done,
    }),
  }
}
