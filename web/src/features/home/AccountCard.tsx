import { ArrowRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { Panel, PanelTitle } from '@/components/Panel'
import { useMe } from '@/features/me/useMe'
import { initials } from '@/lib/format'

export function AccountCard() {
  const { t } = useTranslation(['home', 'common'])
  const me = useMe()
  if (!me.data) return null
  return (
    <Panel aria-labelledby="acct-h">
      <PanelTitle id="acct-h">{t('home:account.title')}</PanelTitle>
      <div className="flex items-center gap-3.5">
        <div
          aria-hidden="true"
          className="flex size-[52px] flex-none items-center justify-center rounded-[14px] bg-pink font-display text-xl font-bold text-on-pink"
        >
          {initials(me.data.display_name)}
        </div>
        <div className="min-w-0">
          <div dir="auto" className="truncate font-display text-xl font-bold">
            {me.data.display_name}
          </div>
          <div dir="ltr" className="truncate text-sm text-muted-foreground rtl:text-end">
            {me.data.email ?? '—'}
          </div>
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <span className="inline-flex items-center gap-1.5 rounded-full bg-pink-soft px-2.5 py-1 text-[13px] font-semibold text-pink-text capitalize">
          {t(`common:role.${me.data.role}`)}
        </span>
        <span className="inline-flex items-center gap-1.5 rounded-full border border-line px-2.5 py-1 text-[13px] font-medium text-muted-foreground capitalize">
          <span className="size-1.5 rounded-full bg-ink" aria-hidden="true" />
          {t(`common:status.${me.data.status}`)}
        </span>
      </div>
      <p className="text-sm text-muted-foreground">{t('home:account.note')}</p>
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-sm font-semibold">
        <Link to="/profile" className="no-underline">
          {t('home:account.goProfile')}
          <ArrowRight className="ms-1 inline size-4 align-text-bottom rtl:-scale-x-100" aria-hidden="true" />
        </Link>
        {me.data.account_url && (
          <a className="no-underline" href={me.data.account_url} target="_blank" rel="noreferrer">
            {t('common:actions.manageAccount')}
          </a>
        )}
      </div>
    </Panel>
  )
}
