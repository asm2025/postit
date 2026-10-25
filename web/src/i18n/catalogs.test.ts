/// <reference types="node" />
import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { AUDIT_KINDS } from '@/features/audit/kinds'

const dir = import.meta.dirname
const read = (p: string): unknown => JSON.parse(readFileSync(p, 'utf8'))

function flatten(value: unknown, prefix = ''): Map<string, string> {
  const out = new Map<string, string>()
  if (value && typeof value === 'object') {
    for (const [k, v] of Object.entries(value)) {
      const key = prefix ? `${prefix}.${k}` : k
      if (typeof v === 'string') out.set(key, v)
      else for (const [ck, cv] of flatten(v, key)) out.set(ck, cv)
    }
  }
  return out
}

function catalog(lang: string): Map<string, string> {
  const all = new Map<string, string>()
  const files = readdirSync(path.join(dir, lang)).filter((f) => f.endsWith('.json'))
  for (const f of files) {
    for (const [k, v] of flatten(read(path.join(dir, lang, f)))) all.set(`${f}:${k}`, v)
  }
  return all
}

// Languages have different plural forms (Arabic has six), so `key_one`/`key_few`... count as one key.
const baseKey = (k: string) => k.replace(/_(zero|one|two|few|many|other)$/, '')
const baseKeys = (m: Map<string, string>) => [...new Set([...m.keys()].map(baseKey))].sort()

it('every key exists in both catalogs', () => {
  const en = catalog('en-US')
  const ar = catalog('ar')
  expect(baseKeys(ar)).toEqual(baseKeys(en))
  for (const [k, v] of en) expect(v.trim(), `en-US ${k}`).not.toBe('')
  for (const [k, v] of ar) expect(v.trim(), `ar ${k}`).not.toBe('')
})

it('every ErrorCode has a translation', () => {
  const spec = read(path.join(dir, '../../../backend/openapi.json')) as {
    components: { schemas: { ErrorCode: { enum: string[] } } }
  }
  const codes = spec.components.schemas.ErrorCode.enum
  expect(codes.length).toBeGreaterThan(0)
  for (const lang of ['en-US', 'ar']) {
    const keys = catalog(lang)
    for (const code of codes) expect(keys.has(`common.json:errors.${code}`), `${lang} errors.${code}`).toBe(true)
  }
})

it('every audit kind has a label in both catalogs', () => {
  expect(AUDIT_KINDS.length).toBeGreaterThan(0)
  for (const lang of ['en-US', 'ar']) {
    const keys = catalog(lang)
    for (const kind of AUDIT_KINDS) expect(keys.has(`audit.json:kinds.${kind}`), `${lang} kinds.${kind}`).toBe(true)
  }
})

it('every ar plural key supplies all CLDR forms', () => {
  const forms = new Intl.PluralRules('ar').resolvedOptions().pluralCategories
  expect(forms).toHaveLength(6)
  const suffix = /_(zero|one|two|few|many|other)$/
  const groups = new Map<string, Set<string>>()
  for (const k of catalog('ar').keys()) {
    const m = suffix.exec(k)
    if (!m) continue
    const base = baseKey(k)
    groups.set(base, (groups.get(base) ?? new Set()).add(m[1]))
  }
  expect(groups.size).toBeGreaterThan(0)
  for (const [base, have] of groups) {
    for (const form of forms) expect(have.has(form), `ar ${base} is missing _${form}`).toBe(true)
  }
})
