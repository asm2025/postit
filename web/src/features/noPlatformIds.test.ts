import { readdirSync, readFileSync } from 'node:fs'
import { join, relative, sep } from 'node:path'

// The web app knows no platform: names, colors, icons, forms, and guides come from `GET /platforms`.
// These are the backend's `PLATFORM_ORDER` (backend/crates/oauth/src/catalog.rs); a platform added
// there is added here too, so a literal of it in the app is caught.
const PLATFORM_IDS = [
  'email',
  'telegram',
  'slack',
  'discord',
  'bluesky',
  'mastodon',
  'x',
  'youtube',
  'linkedin',
  'facebook',
  'instagram',
  'threads',
  'tiktok',
]
// `email` and `x` are ordinary words (`type="email"`, `autoComplete="email"`, a chart axis `'x'`).
const AMBIGUOUS = ['email', 'x']
const DISTINCTIVE = PLATFORM_IDS.filter((id) => !AMBIGUOUS.includes(id))

const quoted = (ids: string[]) => `['"\`](?:${ids.join('|')})['"\`]`
const RULES: { re: RegExp; why: string }[] = [
  // The distinctive IDs fail as any quoted string literal.
  { re: new RegExp(quoted(DISTINCTIVE)), why: 'a platform ID as a string literal' },
  // The ambiguous ones fail only where they name a platform.
  {
    re: new RegExp(`[!=]==?\\s*${quoted(AMBIGUOUS)}|${quoted(AMBIGUOUS)}\\s*[!=]==?`),
    why: 'a comparison with a platform ID',
  },
  { re: new RegExp(`\\bcase\\s+${quoted(AMBIGUOUS)}\\s*:`), why: 'a case label naming a platform' },
  { re: new RegExp(`\\bplatform(?:_id)?\\s*:\\s*${quoted(AMBIGUOUS)}`), why: 'a platform key holding a platform ID' },
]

function offences(text: string): string[] {
  return RULES.filter(({ re }) => re.test(text)).map(({ why }) => why)
}

const SRC = join(__dirname, '..')
const ROOTS = [join(SRC, 'features'), join(SRC, 'components')]

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name)
    if (e.isDirectory()) return files(path)
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [path] : []
  })
}

it('no feature or component names a platform', () => {
  const found = ROOTS.flatMap((root) =>
    files(root).flatMap((path) =>
      offences(readFileSync(path, 'utf8')).map((why) => `${relative(SRC, path).split(sep).join('/')}: ${why}`),
    ),
  )
  expect(found).toEqual([])
})

describe('the rules', () => {
  it('catch a distinctive ID in any quotes', () => {
    expect(offences("const id = 'telegram'")).not.toEqual([])
    expect(offences('const id = "mastodon"')).not.toEqual([])
    expect(offences('const id = `tiktok`')).not.toEqual([])
    expect(offences("if (p.platform === 'slack') {}")).not.toEqual([])
  })

  it('catch email and x only where they name a platform', () => {
    expect(offences("if (a.platform === 'x') {}")).not.toEqual([])
    expect(offences('if ("email" !== id) {}')).not.toEqual([])
    expect(offences("switch (id) { case 'email': break }")).not.toEqual([])
    expect(offences("const a = { platform: 'x' }")).not.toEqual([])
    expect(offences('const a = { platform_id: "email" }')).not.toEqual([])
  })

  it('let ordinary uses of email and x through', () => {
    expect(offences('<input type="email" autoComplete="email" />')).toEqual([])
    expect(offences("const axis = { dataKey: 'x' }")).toEqual([])
    expect(offences("t('delegations:grant.email')")).toEqual([])
    expect(offences("const label = 'Their email'")).toEqual([])
  })

  it('do not match inside a longer word', () => {
    expect(offences("const a = 'telegrams'")).toEqual([])
    expect(offences("const a = 'xylophone'")).toEqual([])
  })
})
