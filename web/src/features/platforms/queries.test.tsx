import { screen, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { vi } from 'vitest'
import { API } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { usePlatformIcon, usePlatforms } from './queries'

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47])

function Icon({ id }: { id: string }) {
  return <p data-testid="icon">{usePlatformIcon(id) ?? 'loading'}</p>
}

function List() {
  const platforms = usePlatforms()
  return <p data-testid="names">{platforms.data?.map((p) => p.display_name).join(',') ?? 'loading'}</p>
}

it('usePlatformIcon returns an object URL and sends the bearer', async () => {
  const create = vi.fn(() => 'blob:icon-1')
  vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: create, revokeObjectURL: vi.fn() }))
  const auth = vi.fn()
  server.use(
    http.get(`${API}/api/v1/platforms/p1/icon`, ({ request }) => {
      auth(request.headers.get('authorization'))
      return new HttpResponse(PNG, { headers: { 'Content-Type': 'image/png' } })
    }),
  )

  renderApp(<Icon id="p1" />)

  expect(await screen.findByText('blob:icon-1')).toBeInTheDocument()
  expect(auth).toHaveBeenCalledWith('Bearer test-token')
  expect(create).toHaveBeenCalledTimes(1)
  vi.unstubAllGlobals()
})

it('usePlatforms lists them without act-as, even in a delegated workspace', async () => {
  const seen = vi.fn()
  server.use(
    http.get(`${API}/api/v1/platforms`, ({ request }) => {
      seen(request.headers.get('x-postit-act-as'))
      return HttpResponse.json([
        {
          platform: 'p1',
          display_name: 'Pixelfed',
          brand_color: '#336699',
          kind: 'broadcast',
          connect: { method: 'oauth' },
          guide: { steps: [] },
        },
      ])
    }),
  )

  renderApp(<List />, { actAs: '0000000a-0000-7000-8000-00000000000a' })

  await waitFor(() => {
    expect(screen.getByTestId('names')).toHaveTextContent('Pixelfed')
  })
  expect(seen).toHaveBeenCalledWith(null)
})
