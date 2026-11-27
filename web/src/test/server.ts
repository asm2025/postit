import { http, HttpResponse } from 'msw'
import { setupServer } from 'msw/node'
import { API, ownWorkspaceFixture } from './fixtures'

// Every page renders inside the workspace provider, which lists the user's workspaces first. A
// test that needs more of them registers its own handler (`server.use`), which wins over this one.
export const server = setupServer(
  http.get(`${API}/api/v1/me/workspaces`, () => HttpResponse.json([ownWorkspaceFixture()])),
  // The bell in the shell polls these; a test that cares registers its own.
  http.get(`${API}/api/v1/notifications/unread-count`, () => HttpResponse.json({ count: 0 })),
  http.get(`${API}/api/v1/notifications`, () => HttpResponse.json({ data: [], page: 1, page_size: 10, total: 0 })),
)
