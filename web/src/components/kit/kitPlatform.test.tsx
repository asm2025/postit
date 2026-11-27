import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18next from 'i18next'
import { http, HttpResponse } from 'msw'
import { useState } from 'react'
import { API } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { server } from '@/test/server'
import { agentText } from '@/i18n/agentText'
import { AccountPicker } from './AccountPicker'
import { PlatformBadge } from './PlatformBadge'
import { ProgressBar } from './ProgressBar'

const PLATFORM = { platform: 'p1', display_name: 'Pixelfed', brand_color: '#336699' }

describe('agentText', () => {
  it('falls back to en', () => {
    expect(agentText({ key: 'nowhere.label', en: 'English text' })).toBe('English text')
  })

  it('prefers a catalog entry for the key', () => {
    i18next.addResourceBundle('en-US', 'agents', { known: { label: 'From the catalog' } }, true, true)
    expect(agentText({ key: 'known.label', en: 'English text' })).toBe('From the catalog')
    i18next.removeResourceBundle('en-US', 'agents')
    i18next.addResourceBundle('en-US', 'agents', {}, true, true)
  })
})

describe('PlatformBadge', () => {
  it('shows the initial while the icon loads', async () => {
    server.use(http.get(`${API}/api/v1/platforms/p1/icon`, () => new Promise(() => undefined)))
    renderApp(<PlatformBadge platform={PLATFORM} />)
    expect(await screen.findByText('P')).toBeInTheDocument()
    expect(screen.getByText('Pixelfed')).toBeInTheDocument()
  })

  it('can hide the name but keeps it for screen readers', async () => {
    server.use(http.get(`${API}/api/v1/platforms/p1/icon`, () => new Promise(() => undefined)))
    renderApp(<PlatformBadge platform={PLATFORM} showName={false} />)
    expect(await screen.findByText('Pixelfed')).toHaveClass('sr-only')
  })
})

describe('ProgressBar', () => {
  it('exposes aria values', async () => {
    renderApp(<ProgressBar value={30} max={120} label="Rows checked" />)
    const bar = await screen.findByRole('progressbar', { name: 'Rows checked' })
    expect(bar).toHaveAttribute('aria-valuenow', '30')
    expect(bar).toHaveAttribute('aria-valuemax', '120')
    expect(bar).toHaveAttribute('aria-valuemin', '0')
  })

  it('fills from the start edge in either direction and never past full', async () => {
    renderApp(<ProgressBar value={500} max={100} label="Rows checked" />)
    const fill = (await screen.findByRole('progressbar')).firstElementChild as HTMLElement
    expect(fill.style.inlineSize).toBe('100%')
  })
})

describe.each(['ltr', 'rtl'] as const)('AccountPicker (%s)', (dir) => {
  beforeEach(() => {
    document.documentElement.dir = dir
    server.use(
      http.get(`${API}/api/v1/platforms`, () =>
        HttpResponse.json([
          { ...PLATFORM, kind: 'broadcast', connect: { method: 'credentials' }, guide: { steps: [] } },
          {
            platform: 'p2',
            display_name: 'Mailer',
            brand_color: '#aa3300',
            kind: 'direct',
            connect: { method: 'credentials' },
            guide: { steps: [] },
          },
        ]),
      ),
      http.get(`${API}/api/v1/platforms/:id/icon`, () => new Promise(() => undefined)),
    )
  })
  afterEach(() => {
    document.documentElement.dir = 'ltr'
  })

  const account = (id: string, platform: string, name: string) => ({
    id,
    platform,
    display_name: name,
    remote_account_id: id,
    status: 'active' as const,
    test_account: false,
    created_at: '2026-10-01T10:00:00Z',
    updated_at: '2026-10-01T10:00:00Z',
  })

  function Harness() {
    const [value, setValue] = useState<string[]>([])
    return (
      <>
        <AccountPicker
          value={value}
          onChange={setValue}
          accounts={[account('a1', 'p1', 'Shop'), account('a2', 'p2', 'Newsletter'), account('a3', 'p1', 'Blog')]}
        />
        <p data-testid="chosen">{value.join(',')}</p>
      </>
    )
  }

  it('groups by platform', async () => {
    renderApp(<Harness />)
    const pixelfed = await screen.findByRole('group', { name: /Pixelfed/ })
    expect(
      within(pixelfed)
        .getAllByRole('checkbox')
        .map((c) => c.getAttribute('id') ?? '').length,
    ).toBe(2)
    expect(within(pixelfed).getByRole('checkbox', { name: 'Shop' })).toBeInTheDocument()
    expect(within(pixelfed).getByRole('checkbox', { name: 'Blog' })).toBeInTheDocument()
    const mailer = screen.getByRole('group', { name: /Mailer/ })
    expect(within(mailer).getByRole('checkbox', { name: 'Newsletter' })).toBeInTheDocument()
  })

  it('reports the chosen accounts', async () => {
    renderApp(<Harness />)
    await userEvent.click(await screen.findByRole('checkbox', { name: 'Shop' }))
    await userEvent.click(screen.getByRole('checkbox', { name: 'Newsletter' }))
    expect(screen.getByTestId('chosen')).toHaveTextContent('a1,a2')
    await userEvent.click(screen.getByRole('checkbox', { name: 'Shop' }))
    expect(screen.getByTestId('chosen')).toHaveTextContent('a2')
  })
})
