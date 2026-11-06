/// <reference types="node" />
import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { AUDIT_KINDS } from '@/features/audit/kinds'
import { SUPPORTED } from './locales'

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

// Languages have different plural forms, so `key_one`/`key_few`... count as one key.
const baseKey = (k: string) => k.replace(/_(zero|one|two|few|many|other)$/, '')
const baseKeys = (m: Map<string, string>) => [...new Set([...m.keys()].map(baseKey))].sort()

it('ships one catalog folder per supported language', () => {
  const folders = readdirSync(dir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    .sort()
  expect(folders).toEqual([...SUPPORTED].sort())
})

it('every key exists in every catalog', () => {
  const en = catalog('en-US')
  for (const lang of SUPPORTED) {
    const c = catalog(lang)
    expect(baseKeys(c), lang).toEqual(baseKeys(en))
    for (const [k, v] of c) expect(v.trim(), `${lang} ${k}`).not.toBe('')
  }
})

it('every ErrorCode has a translation', () => {
  const spec = read(path.join(dir, '../../../backend/openapi.json')) as {
    components: { schemas: { ErrorCode: { enum: string[] } } }
  }
  const codes = spec.components.schemas.ErrorCode.enum
  expect(codes.length).toBeGreaterThan(0)
  for (const lang of SUPPORTED) {
    const keys = catalog(lang)
    for (const code of codes) expect(keys.has(`common.json:errors.${code}`), `${lang} errors.${code}`).toBe(true)
  }
})

it('every audit kind has a label in every catalog', () => {
  expect(AUDIT_KINDS.length).toBeGreaterThan(0)
  for (const lang of SUPPORTED) {
    const keys = catalog(lang)
    for (const kind of AUDIT_KINDS) expect(keys.has(`audit.json:kinds.${kind}`), `${lang} kinds.${kind}`).toBe(true)
  }
})

it('every plural key supplies all CLDR forms of its language', () => {
  const suffix = /_(zero|one|two|few|many|other)$/
  for (const lang of SUPPORTED) {
    const forms = new Intl.PluralRules(lang).resolvedOptions().pluralCategories
    const groups = new Map<string, Set<string>>()
    for (const k of catalog(lang).keys()) {
      const m = suffix.exec(k)
      if (!m) continue
      const base = baseKey(k)
      groups.set(base, (groups.get(base) ?? new Set()).add(m[1]))
    }
    for (const [base, have] of groups) {
      for (const form of forms) expect(have.has(form), `${lang} ${base} is missing _${form}`).toBe(true)
    }
  }
})
