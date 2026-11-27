import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { needsActAs } from './actAs'

const API = 'http://api.test'

// [path, whether the request carries X-Postit-Act-As in a delegated workspace]
const TABLE: [string, boolean][] = [
  ['/api/v1/accounts', true],
  ['/api/v1/media', true],
  ['/api/v1/media/uploads/x', true],
  ['/api/v1/delegations', true],
  ['/api/v1/target-groups', true],
  ['/api/v1/me', false],
  ['/api/v1/me/workspaces', false],
  ['/api/v1/notifications/unread-count', false],
  ['/api/v1/notifications/x', false],
  ['/api/v1/push/vapid-public-key', false],
  ['/api/v1/users', false],
  ['/api/v1/users/x', false],
  ['/api/v1/users/lookup?email=a', false],
  ['/api/v1/delegations/received/x', false],
  ['/api/v1/admin/platforms', false],
  ['/api/v1/auth/config', false],
  ['/api/v1/platforms/x/icon', false],
  ['/api/v1/ai/presets', false],
  ['/api/v1/ai/providers', true],
  ['/config.json', false],
  // Whole segments only: a prefix is not a match.
  ['/api/v1/meta', true],
  ['/api/v1/usersettings', true],
]

it.each(TABLE)('%s carries act-as: %s', (path, expected) => {
  expect(needsActAs(`${API}${path}`)).toBe(expected)
})

it('is false for a URL that is not one', () => {
  expect(needsActAs('not a url')).toBe(false)
})

// The pair of routes that list `delegation_invalid` but answer the same in every workspace.
const WORKSPACE_INDEPENDENT = [/^\/api\/v1\/platforms(\/|$)/, /^\/api\/v1\/ai\/presets$/]

describe('against backend/openapi.json', () => {
  const file = join(__dirname, '..', '..', '..', 'backend', 'openapi.json')
  const present = existsSync(file)

  interface Op {
    responses?: Record<string, { description?: string }>
  }
  const operations = (): [string, string, Op][] => {
    const spec = JSON.parse(readFileSync(file, 'utf8')) as { paths: Record<string, Record<string, Op>> }
    return Object.entries(spec.paths).flatMap(([path, methods]) =>
      Object.entries(methods)
        .filter(([method]) => ['get', 'post', 'put', 'patch', 'delete'].includes(method))
        .map(([method, op]): [string, string, Op] => [method.toUpperCase(), path, op]),
    )
  }
  const url = (path: string) => `${API}${path.replace(/\{[^}]+\}/g, 'x')}`

  // Skipped, with this name saying why, where the backend folder is not checked out beside web/.
  it.skipIf(!present)('never sends the header to a route that refuses it', () => {
    const wrong = operations().filter(([, path, op]) => {
      const text = op.responses?.['403']?.description ?? ''
      const refuses = text.includes('does not accept it') || text.includes('`owner_only` (`X-Postit-Act-As` sent')
      return refuses && needsActAs(url(path))
    })
    expect(wrong.map(([method, path]) => `${method} ${path}`)).toEqual([])
  })

  it.skipIf(!present)('always sends the header to a workspace route', () => {
    const wrong = operations().filter(([, path, op]) => {
      const text = op.responses?.['403']?.description ?? ''
      return (
        text.includes('delegation_invalid') &&
        !WORKSPACE_INDEPENDENT.some((re) => re.test(path)) &&
        !needsActAs(url(path))
      )
    })
    expect(wrong.map(([method, path]) => `${method} ${path}`)).toEqual([])
  })
})
