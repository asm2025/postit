import { createContext, useContext } from 'react'
import type { AuthConfig, RuntimeConfig } from './runtime'

export interface AppConfig {
  runtime: RuntimeConfig
  auth: AuthConfig
}

export const ConfigContext = createContext<AppConfig | null>(null)

export function useConfig(): AppConfig {
  const c = useContext(ConfigContext)
  if (!c) throw new Error('ConfigContext missing')
  return c
}
