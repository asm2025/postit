import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
import { vi } from 'vitest'
import { fromLocalInput } from './datetime'
import { SchemaForm } from './SchemaForm'
import { initialValues, parseSchema, submittedValues, type SchemaFieldSpec } from './schema'

const m = (key: string, en: string) => ({ key, en })

function schema(properties: Record<string, Record<string, unknown>>, required: string[] = []) {
  return { type: 'object', properties, required, additionalProperties: false }
}

function Harness({
  fields,
  initial,
  errors,
  onValues,
  disabled,
}: {
  fields: SchemaFieldSpec[]
  initial?: Record<string, unknown>
  errors?: Record<string, string>
  onValues?: (values: Record<string, unknown>) => void
  disabled?: boolean
}) {
  const [values, setValues] = useState(initial ?? initialValues(fields))
  return (
    <SchemaForm
      fields={fields}
      values={values}
      errors={errors}
      disabled={disabled}
      onChange={(next) => {
        setValues(next)
        onValues?.(next)
      }}
    />
  )
}

function draw(props: Parameters<typeof Harness>[0]) {
  return render(<Harness {...props} />)
}

describe.each(['ltr', 'rtl'] as const)('SchemaForm (%s)', (dir) => {
  beforeEach(() => {
    document.documentElement.dir = dir
  })
  afterEach(() => {
    document.documentElement.dir = 'ltr'
  })

  it('renders string with limits', async () => {
    const fields = parseSchema(
      schema({ name: { type: 'string', minLength: 2, maxLength: 10, 'x-postit-label': m('x.name', 'Name') } }, [
        'name',
      ]),
    )
    const seen = vi.fn()
    draw({ fields, onValues: seen })
    const input = screen.getByRole('textbox', { name: /Name/ })
    expect(input).toHaveAttribute('maxlength', '10')
    expect(input).toHaveAttribute('minlength', '2')
    expect(input).toBeRequired()
    await userEvent.type(input, 'Ada')
    expect(seen).toHaveBeenLastCalledWith({ name: 'Ada' })
  })

  it('multiline', async () => {
    const fields = parseSchema(
      schema({
        body: { type: 'string', maxLength: 50, 'x-postit-multiline': true, 'x-postit-label': m('x.body', 'Body') },
      }),
    )
    const seen = vi.fn()
    draw({ fields, onValues: seen })
    const area = screen.getByRole('textbox', { name: 'Body' })
    expect(area.tagName).toBe('TEXTAREA')
    expect(area).toHaveAttribute('maxlength', '50')
    await userEvent.type(area, 'hi')
    expect(seen).toHaveBeenLastCalledWith({ body: 'hi' })
  })

  it('enum with labels', async () => {
    const fields = parseSchema(
      schema(
        {
          kind: {
            type: 'string',
            enum: ['a', 'b'],
            default: 'a',
            'x-postit-label': m('x.kind', 'Kind'),
            'x-postit-enum-labels': [m('x.kind.a', 'Alpha'), m('x.kind.b', 'Bravo')],
          },
        },
        ['kind'],
      ),
    )
    const seen = vi.fn()
    draw({ fields, onValues: seen })
    expect(screen.getByRole('combobox', { name: 'Kind' })).toHaveTextContent('Alpha')
    await userEvent.click(screen.getByRole('combobox', { name: 'Kind' }))
    await userEvent.click(await screen.findByRole('option', { name: 'Bravo' }))
    expect(seen).toHaveBeenLastCalledWith({ kind: 'b' })
  })

  it('boolean', async () => {
    const fields = parseSchema(
      schema({ silent: { type: 'boolean', default: false, 'x-postit-label': m('x.s', 'Silent') } }),
    )
    const seen = vi.fn()
    draw({ fields, onValues: seen })
    const toggle = screen.getByRole('switch', { name: 'Silent' })
    expect(toggle).not.toBeChecked()
    await userEvent.click(toggle)
    expect(seen).toHaveBeenLastCalledWith({ silent: true })
  })

  it('consent must be checked', async () => {
    const fields = parseSchema(
      schema(
        {
          consent: {
            type: 'boolean',
            'x-postit-label': m('x.c', 'Consent'),
            'x-postit-consent-text': m('x.c.t', 'I have permission to email these people.'),
            'x-postit-no-default': true,
          },
        },
        ['consent'],
      ),
    )
    const seen = vi.fn()
    draw({ fields, onValues: seen })
    const box = screen.getByRole('checkbox', { name: 'I have permission to email these people.' })
    expect(box).not.toBeChecked()
    await userEvent.click(box)
    expect(seen).toHaveBeenLastCalledWith({ consent: true })
  })

  it('integer bounds', async () => {
    const fields = parseSchema(
      schema({ port: { type: 'integer', minimum: 1, maximum: 65535, 'x-postit-label': m('x.p', 'Port') } }, ['port']),
    )
    const seen = vi.fn()
    draw({ fields, onValues: seen })
    const input = screen.getByRole('spinbutton', { name: /Port/ })
    expect(input).toHaveAttribute('min', '1')
    expect(input).toHaveAttribute('max', '65535')
    await userEvent.type(input, '587')
    expect(seen).toHaveBeenLastCalledWith({ port: 587 })
  })

  it("datetime round trips in the user's zone", async () => {
    const fields = parseSchema(
      schema({ at: { type: 'string', format: 'date-time', 'x-postit-label': m('x.at', 'When') } }),
    )
    const seen = vi.fn()
    draw({ fields, onValues: seen })
    const input = screen.getByLabelText('When')
    await userEvent.type(input, '2026-12-01T10:30')
    const sent = seen.mock.calls.at(-1)?.[0] as { at: string }
    // RFC 3339 with the offset of the zone the user typed it in.
    expect(sent.at).toBe(fromLocalInput('2026-12-01T10:30'))
    expect(sent.at).toMatch(/^2026-12-01T10:30:00[+-]\d{2}:\d{2}$/)
    // And it reads back as what the user typed.
    expect(input).toHaveValue('2026-12-01T10:30')
  })

  it('secret is masked, toggles, and is never prefilled', async () => {
    const fields = parseSchema(
      schema({ token: { type: 'string', 'x-postit-secret': true, 'x-postit-label': m('x.t', 'Token') } }, ['token']),
    )
    expect(initialValues(fields)).toEqual({})
    draw({ fields })
    const input = screen.getByLabelText(/Token/)
    expect(input).toHaveAttribute('type', 'password')
    expect(input).toHaveAttribute('autocomplete', 'off')
    expect(input).toHaveValue('')
    await userEvent.type(input, 'abc')
    await userEvent.click(screen.getByRole('button', { name: 'Show' }))
    expect(input).toHaveAttribute('type', 'text')
    expect(input).toHaveValue('abc')
    await userEvent.click(screen.getByRole('button', { name: 'Hide' }))
    expect(input).toHaveAttribute('type', 'password')
  })

  it('help text', () => {
    const fields = parseSchema(
      schema({
        host: {
          type: 'string',
          'x-postit-label': m('x.h', 'Host'),
          'x-postit-help': m('x.h.help', 'The SMTP server.'),
        },
      }),
    )
    draw({ fields })
    expect(screen.getByRole('textbox', { name: 'Host' })).toHaveAccessibleDescription('The SMTP server.')
  })

  it('shows an error under its field', () => {
    const fields = parseSchema(schema({ host: { type: 'string', 'x-postit-label': m('x.h', 'Host') } }))
    draw({ fields, errors: { host: 'Enter a host.' } })
    expect(screen.getByRole('textbox', { name: 'Host' })).toBeInvalid()
    expect(screen.getByRole('alert')).toHaveTextContent('Enter a host.')
  })

  it('noDefault starts empty and counts as required', () => {
    const fields = parseSchema(
      schema({
        tls: {
          type: 'string',
          enum: ['a', 'b'],
          'x-postit-label': m('x.tls', 'TLS'),
          'x-postit-enum-labels': [m('a', 'A'), m('b', 'B')],
          'x-postit-no-default': true,
        },
      }),
    )
    expect(fields[0].required).toBe(true)
    expect(initialValues(fields)).toEqual({})
    draw({ fields })
    expect(screen.getByText('*')).toBeInTheDocument()
  })

  it('depends-on hides and drops the field', async () => {
    const fields = parseSchema(
      schema({
        mode: {
          type: 'string',
          enum: ['plain', 'tls'],
          default: 'plain',
          'x-postit-label': m('x.m', 'Mode'),
          'x-postit-enum-labels': [m('p', 'Plain'), m('t', 'TLS')],
        },
        cert: {
          type: 'string',
          'x-postit-label': m('x.c', 'Certificate'),
          'x-postit-depends-on': { field: 'mode', equals: 'tls' },
        },
      }),
    )
    const seen = vi.fn()
    draw({ fields, initial: { mode: 'plain', cert: 'stale' }, onValues: seen })
    expect(screen.queryByRole('textbox', { name: 'Certificate' })).not.toBeInTheDocument()
    expect(submittedValues(fields, { mode: 'plain', cert: 'stale' })).toEqual({ mode: 'plain' })

    await userEvent.click(screen.getByRole('combobox', { name: 'Mode' }))
    await userEvent.click(await screen.findByRole('option', { name: 'TLS' }))
    expect(screen.getByRole('textbox', { name: 'Certificate' })).toBeInTheDocument()
    expect(submittedValues(fields, { mode: 'tls', cert: 'c' })).toEqual({ mode: 'tls', cert: 'c' })
  })

  it('disables every control', () => {
    const fields = parseSchema(schema({ host: { type: 'string', 'x-postit-label': m('x.h', 'Host') } }))
    draw({ fields, disabled: true })
    expect(screen.getByRole('textbox', { name: 'Host' })).toBeDisabled()
  })
})

describe('parseSchema', () => {
  it('keeps declaration order', () => {
    const fields = parseSchema(
      schema({
        zeta: { type: 'string', 'x-postit-label': m('z', 'Zeta') },
        alpha: { type: 'string', 'x-postit-label': m('a', 'Alpha') },
      }),
    )
    expect(fields.map((f) => f.name)).toEqual(['zeta', 'alpha'])
  })

  it('unknown keyword skips the field', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const fields = parseSchema(
      schema({
        ok: { type: 'string', 'x-postit-label': m('o', 'Ok') },
        odd: { type: 'string', pattern: '^a', 'x-postit-label': m('d', 'Odd') },
      }),
    )
    expect(fields.map((f) => f.name)).toEqual(['ok'])
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('odd'))
    warn.mockRestore()
  })

  it('submittedValues drops empty optional strings only', () => {
    const fields = parseSchema(
      schema(
        {
          a: { type: 'string', 'x-postit-label': m('a', 'A') },
          b: { type: 'string', 'x-postit-label': m('b', 'B') },
        },
        ['b'],
      ),
    )
    expect(submittedValues(fields, { a: '', b: '' })).toEqual({ b: '' })
  })

  it('initialValues takes defaults and skips secrets', () => {
    const fields = parseSchema(
      schema({
        a: { type: 'string', default: 'x', 'x-postit-label': m('a', 'A') },
        s: { type: 'string', default: 'hidden', 'x-postit-secret': true, 'x-postit-label': m('s', 'S') },
      }),
    )
    expect(initialValues(fields)).toEqual({ a: 'x' })
  })
})
