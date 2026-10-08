import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useMe } from '@/features/me/useMe'

export function AccountCard() {
  const me = useMe()
  if (!me.data) return null
  return (
    <Card className="max-w-md">
      <CardHeader>
        <CardTitle>Your account</CardTitle>
      </CardHeader>
      <CardContent className="space-y-1 text-sm">
        <div className="text-base font-medium">{me.data.display_name}</div>
        <div className="text-muted-foreground">{me.data.email ?? '—'}</div>
        <div className="flex gap-2 pt-2">
          <Badge>{me.data.role}</Badge>
          <Badge variant="secondary">{me.data.status}</Badge>
        </div>
        {me.data.account_url && (
          <a className="block pt-2 underline" href={me.data.account_url} target="_blank" rel="noreferrer">
            Manage account
          </a>
        )}
      </CardContent>
    </Card>
  )
}
