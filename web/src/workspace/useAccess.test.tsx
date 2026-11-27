import { screen } from '@testing-library/react'
import { delegatedWorkspaceFixture, ownWorkspaceFixture, type Workspace } from '@/test/fixtures'
import { renderApp } from '@/test/render'
import { useAccess } from './useAccess'

const OWNER = '0000000a-0000-7000-8000-00000000000a'
const IN_LIMIT = '11111111-1111-7111-8111-111111111111'
const OUT_OF_LIMIT = '22222222-2222-7222-8222-222222222222'

function Probe() {
  const access = useAccess()
  return (
    <p data-testid="access">
      {JSON.stringify({
        own: access.own,
        level: access.level,
        view: access.atLeast('view'),
        edit: access.atLeast('edit'),
        publish: access.atLeast('publish'),
        inLimit: access.inLimit(IN_LIMIT),
        outOfLimit: access.inLimit(OUT_OF_LIMIT),
      })}
    </p>
  )
}

async function read(workspace: Workspace | null) {
  renderApp(<Probe />, workspace ? { actAs: OWNER, workspaces: [ownWorkspaceFixture(), workspace] } : {})
  const text = (await screen.findByTestId('access')).textContent
  return JSON.parse(text) as Record<string, unknown>
}

it('own workspace: owner, everything allowed', async () => {
  expect(await read(null)).toEqual({
    own: true,
    level: 'owner',
    view: true,
    edit: true,
    publish: true,
    inLimit: true,
    outOfLimit: true,
  })
})

it('view reaches view only', async () => {
  const access = await read(delegatedWorkspaceFixture(OWNER, { level: 'view' }))
  expect(access).toMatchObject({ own: false, level: 'view', view: true, edit: false, publish: false, inLimit: true })
})

it('edit reaches view and edit', async () => {
  const access = await read(delegatedWorkspaceFixture(OWNER, { level: 'edit' }))
  expect(access).toMatchObject({ level: 'edit', view: true, edit: true, publish: false })
})

it('publish reaches everything a delegate can do', async () => {
  const access = await read(delegatedWorkspaceFixture(OWNER, { level: 'publish' }))
  expect(access).toMatchObject({ level: 'publish', view: true, edit: true, publish: true })
})

it('a limited publish grant reaches only the listed accounts', async () => {
  const access = await read(
    delegatedWorkspaceFixture(OWNER, { level: 'publish', all_accounts: false, account_ids: [IN_LIMIT] }),
  )
  expect(access).toMatchObject({ level: 'publish', inLimit: true, outOfLimit: false })
})
