import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { messageFor } from '@/api/errors'
import { ErrorText } from '@/components/kit/ErrorText'
import { FormField } from '@/components/kit/FormField'
import { OptionSelect } from '@/components/kit/OptionSelect'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { useSaveWritingStyle, type WritingStyle } from './queries'

const EMOJI: WritingStyle['emoji'][] = ['none', 'light', 'heavy']
const HASHTAGS: WritingStyle['hashtags'][] = ['none', 'few', 'many']

/** How the drafter writes for one account. Read-only unless `editable`: delegates below `edit`, or outside their limit. */
export function WritingStyleForm({
  accountId,
  style,
  editable,
}: {
  accountId: string
  style: WritingStyle
  editable: boolean
}) {
  const { t } = useTranslation(['accounts', 'common'])
  const save = useSaveWritingStyle(accountId)
  const [tone, setTone] = useState(style.tone)
  const [emoji, setEmoji] = useState(style.emoji)
  const [hashtags, setHashtags] = useState(style.hashtags)
  const [language, setLanguage] = useState(style.language ?? '')
  const [notes, setNotes] = useState(style.notes)

  return (
    <section aria-labelledby="style-title" className="flex max-w-lg flex-col gap-4">
      <div>
        <h2 id="style-title" className="font-display text-xl font-semibold">
          {t('accounts:style.title')}
        </h2>
        <p className="text-sm text-muted-foreground">
          {editable ? t('accounts:style.intro') : t('accounts:style.readOnly')}
        </p>
      </div>
      <FormField label={t('accounts:style.tone')}>
        <Input
          value={tone}
          disabled={!editable}
          dir="auto"
          onChange={(e) => {
            setTone(e.target.value)
          }}
        />
      </FormField>
      <FormField label={t('accounts:style.emoji')}>
        <OptionSelect
          aria-label={t('accounts:style.emoji')}
          value={emoji}
          disabled={!editable}
          options={EMOJI.map((v) => ({ value: v, label: t(`accounts:style.emojiOption.${v}`) }))}
          onChange={(v) => {
            setEmoji(v as WritingStyle['emoji'])
          }}
        />
      </FormField>
      <FormField label={t('accounts:style.hashtags')}>
        <OptionSelect
          aria-label={t('accounts:style.hashtags')}
          value={hashtags}
          disabled={!editable}
          options={HASHTAGS.map((v) => ({ value: v, label: t(`accounts:style.hashtagOption.${v}`) }))}
          onChange={(v) => {
            setHashtags(v as WritingStyle['hashtags'])
          }}
        />
      </FormField>
      <FormField label={t('accounts:style.language')}>
        <Input
          value={language}
          disabled={!editable}
          dir="ltr"
          placeholder="en-US"
          onChange={(e) => {
            setLanguage(e.target.value)
          }}
        />
      </FormField>
      <FormField label={t('accounts:style.notes')}>
        <Textarea
          value={notes}
          disabled={!editable}
          dir="auto"
          onChange={(e) => {
            setNotes(e.target.value)
          }}
        />
      </FormField>
      <ErrorText error={save.error ? messageFor(save.error) : null} />
      {editable && (
        <Button
          className="self-start"
          disabled={save.isPending}
          onClick={() => {
            save.mutate({ tone, emoji, hashtags, language: language.trim() === '' ? null : language.trim(), notes })
          }}
        >
          {save.isSuccess ? t('accounts:style.saved') : t('common:actions.save')}
        </Button>
      )}
    </section>
  )
}
