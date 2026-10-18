import { readdirSync, readFileSync } from 'node:fs'
import { join, relative, sep } from 'node:path'

// Pages build lists from the kit. Anything here outside `components/kit` and `components/ui` is a page
// drawing its own table or picker, which is how copies drift apart.
const RULES: [RegExp, string][] = [
  [/<table[\s>]/, 'renders a <table>; use DataTable'],
  [/from '@\/components\/ui\/table'/, 'imports the table primitives; use DataTable'],
  [/from '@tanstack\/react-table'/, 'uses TanStack Table directly; use DataTable and tableColumns'],
  [/<select[\s>]/, 'renders a native <select>; use OptionSelect'],
]

const SRC = join(__dirname, '..', '..')
const ALLOWED = [join('components', 'kit') + sep, join('components', 'ui') + sep]

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name)
    if (e.isDirectory()) return files(path)
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [path] : []
  })
}

it('no page draws its own table, table footer, or select', () => {
  const found = files(SRC).flatMap((path) => {
    const rel = relative(SRC, path)
    if (ALLOWED.some((a) => rel.startsWith(a))) return []
    const text = readFileSync(path, 'utf8')
    return RULES.filter(([re]) => re.test(text)).map(([, why]) => `${rel.split(sep).join('/')} ${why}`)
  })
  expect(found).toEqual([])
})
