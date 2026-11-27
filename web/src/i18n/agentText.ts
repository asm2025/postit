import i18next from 'i18next'

/** A message an agent declares: a stable key and its English text. */
export interface AgentMessage {
  key: string
  en: string
}

/**
 * The text of an agent message: the catalog's translation when `agents:<key>` has one, the
 * agent's own English otherwise. Agents ship English only; a translation is a catalog entry.
 */
export function agentText(message: AgentMessage): string {
  const key = `agents:${message.key}`
  if (!i18next.exists(key)) return message.en
  const translate = i18next.t.bind(i18next) as (key: string) => string
  return translate(key)
}
