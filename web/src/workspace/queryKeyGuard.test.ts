import { readdirSync, readFileSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { SELF_KEYS } from './keys'

// A workspace's data lives under a key built by `wsKey`, so switching workspaces can never show
// one's data in another. A literal key is allowed only when the data is the same in every
// workspace, which is what `SELF_KEYS` lists.
const ROOTS = [join(__dirname, '..', 'features'), __dirname]

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name)
    if (e.isDirectory()) return files(path)
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [path] : []
  })
}

const LITERAL = /queryKey:\s*\[\s*([^\],]*)/g

function offenders(text: string): string[] {
  return [...text.matchAll(LITERAL)].flatMap(([, first]) => {
    const quoted = /^(['"`])(.*)\1$/.exec(first.trim())
    return quoted && SELF_KEYS.has(quoted[2]) ? [] : [first.trim()]
  })
}

it('every literal query key starts with a self or admin key', () => {
  const found = ROOTS.flatMap((root) =>
    files(root).flatMap((path) =>
      offenders(readFileSync(path, 'utf8')).map(
        (first) => `${relative(join(__dirname, '..'), path).split(sep).join('/')}: queryKey: [${first}`,
      ),
    ),
  )
  expect(found).toEqual([])
})

it('catches a workspace key written as a literal', () => {
  expect(offenders("useQuery({ queryKey: ['accounts', id] })")).toEqual(["'accounts'"])
  expect(offenders("useQuery({ queryKey: [ownerId, 'accounts'] })")).toEqual(['ownerId'])
  expect(offenders("useQuery({ queryKey: ['users', 'list'] })")).toEqual([])
  expect(offenders("useQuery({ queryKey: ws('accounts') })")).toEqual([])
})
