import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useApi } from '@/api/ApiContext'
import { unwrap } from '@/api/client'
import type { components } from '@/api/schema'
import { useWsKey } from '@/workspace/keys'

export type AccountImport = components['schemas']['AccountImportDto']
export type ImportStatus = components['schemas']['ImportStatusDto']
export type ImportStop = components['schemas']['ImportStopDto']

const POLL_MS = 2000

/**
 * One import. Polled every two seconds while the job runs and not at all once it is done or
 * waiting for the owner, so a finished page costs nothing.
 */
export function useImport(id: string | null) {
  const api = useApi()
  const key = useWsKey()
  return useQuery({
    queryKey: key('accounts', 'import', id),
    queryFn: () => unwrap(api.GET('/api/v1/accounts/imports/{id}', { params: { path: { id: id ?? '' } } })),
    enabled: id !== null,
    retry: false,
    refetchInterval: (query) => (query.state.data?.status === 'processing' ? POLL_MS : false),
  })
}

/** Sends the file to be checked (`commit` false) or connected (`commit` true); answers with the import. */
export function useStartImport() {
  const api = useApi()
  const qc = useQueryClient()
  const key = useWsKey()
  return useMutation({
    mutationFn: ({ platform, file, commit }: { platform: string; file: File; commit: boolean }) =>
      unwrap(
        api.POST('/api/v1/accounts/import/{platform}', {
          params: { path: { platform }, query: { commit } },
          // The file goes as it is, not as JSON.
          body: file as never,
          bodySerializer: (body: unknown) => body as BodyInit,
          headers: { 'Content-Type': 'text/csv' },
        }),
      ),
    onSuccess: (started) => {
      qc.setQueryData(key('accounts', 'import', started.id), started)
    },
  })
}

/** Connects the rows a commit run read before its file stopped early. */
export function useContinueImport() {
  const api = useApi()
  const qc = useQueryClient()
  const key = useWsKey()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api.POST('/api/v1/accounts/imports/{id}/continue', { params: { path: { id } } })),
    onSuccess: (resumed) => {
      qc.setQueryData(key('accounts', 'import', resumed.id), resumed)
    },
  })
}

/** Cancels a commit run that stopped early: nothing is connected and the rows read are deleted. */
export function useCancelImport() {
  const api = useApi()
  const qc = useQueryClient()
  const key = useWsKey()
  return useMutation({
    mutationFn: (id: string) => unwrap(api.DELETE('/api/v1/accounts/imports/{id}', { params: { path: { id } } })),
    onSuccess: (canceled) => {
      qc.setQueryData(key('accounts', 'import', canceled.id), canceled)
    },
  })
}
