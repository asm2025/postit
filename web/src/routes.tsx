import { createBrowserRouter } from 'react-router'
import { Shell } from '@/components/Shell'
import { CallbackPage } from '@/features/me/CallbackPage'
import { Gate, RequireAdmin, RequireSession } from '@/features/me/Gate'
import { LoginPage } from '@/features/me/LoginPage'
import { ProfilePage } from '@/features/me/ProfilePage'
import { AuditPage } from '@/features/audit/AuditPage'
import { HomePage } from '@/features/home/HomePage'
import { UsersPage } from '@/features/users/UsersPage'

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
              element: <Shell />,
              children: [
                { index: true, element: <HomePage /> },
                { path: 'profile', element: <ProfilePage /> },
                {
                  element: <RequireAdmin />,
                  children: [
                    { path: 'users', element: <UsersPage /> },
                    { path: 'audit', element: <AuditPage /> },
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
