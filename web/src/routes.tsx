import { createBrowserRouter } from 'react-router'
import { Shell } from '@/components/Shell'
import { CallbackPage } from '@/features/me/CallbackPage'
import { Gate, RequireAdmin, RequireSession } from '@/features/me/Gate'
import { LoginPage } from '@/features/me/LoginPage'
import { ProfilePage } from '@/features/me/ProfilePage'
import { AuditPage } from '@/features/audit/AuditPage'
import { DelegationsPage } from '@/features/delegations/DelegationsPage'
import { AccountDetailPage } from '@/features/accounts/AccountDetailPage'
import { OAuthDonePage } from '@/features/accounts/connect/OAuthDonePage'
import { ConnectPage } from '@/features/accounts/connect/ConnectPage'
import { AccountsPage } from '@/features/accounts/AccountsPage'
import { HomePage } from '@/features/home/HomePage'
import { NotificationsPage } from '@/features/notifications/NotificationsPage'
import { JobDetailPage } from '@/features/jobs/JobDetailPage'
import { JobsDashboard } from '@/features/jobs/JobsDashboard'
import { JobsList } from '@/features/jobs/JobsList'
import { RecurringPage } from '@/features/jobs/RecurringPage'
import { UsersPage } from '@/features/users/UsersPage'
import { RequireOwn } from '@/workspace/RequireOwn'
import { WorkspaceProvider } from '@/workspace/WorkspaceProvider'

export function createRouter() {
  return createBrowserRouter([
    { path: '/login', element: <LoginPage /> },
    { path: '/auth/callback', element: <CallbackPage /> },
    {
      element: <RequireSession />,
      children: [
        {
          element: <Gate />,
          children: [
            {
              element: <WorkspaceProvider />,
              children: [
                {
                  element: <Shell />,
                  children: [
                    { index: true, element: <HomePage /> },
                    { path: 'profile', element: <ProfilePage /> },
                    { path: 'notifications', element: <NotificationsPage /> },
                    { path: 'delegations', element: <DelegationsPage /> },
                    { path: 'accounts', element: <AccountsPage /> },
                    { path: 'oauth-done', element: <OAuthDonePage /> },
                    {
                      element: <RequireOwn />,
                      children: [
                        { path: 'accounts/connect', element: <ConnectPage /> },
                        { path: 'accounts/connect/:platform', element: <ConnectPage /> },
                      ],
                    },
                    { path: 'accounts/:id', element: <AccountDetailPage /> },
                    {
                      element: <RequireAdmin />,
                      children: [
                        { path: 'users', element: <UsersPage /> },
                        { path: 'audit', element: <AuditPage /> },
                        { path: 'jobs', element: <JobsDashboard /> },
                        { path: 'jobs/list', element: <JobsList /> },
                        { path: 'jobs/recurring', element: <RecurringPage /> },
                        { path: 'jobs/:id', element: <JobDetailPage /> },
                      ],
                    },
                  ],
                },
              ],
            },
          ],
        },
      ],
    },
  ])
}
