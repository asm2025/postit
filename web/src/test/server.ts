import { http, HttpResponse } from 'msw'
import { setupServer } from 'msw/node'
import { API, ownWorkspaceFixture } from './fixtures'

// Every page renders inside the workspace provider, which lists the user's workspaces first. A
// test that needs more of them registers its own handler (`server.use`), which wins over this one.
export const server = setupServer(
  http.get(`${API}/api/v1/me/workspaces`, () => HttpResponse.json([ownWorkspaceFixture()])),
)
