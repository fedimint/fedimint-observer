import { readFileSync } from 'node:fs'
import { defineConfig, loadEnv, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'

// Serves llms.txt with the API host this build talks to, from
// VITE_FMO_API_BASE_URL. Without it, lines naming the API are left out rather
// than pointing at a guessed host.
function llmsTxt(apiBase: string | undefined): Plugin {
  const render = () => readFileSync(new URL('./llms.txt', import.meta.url), 'utf8')
    .split('\n')
    .filter((line) => apiBase || !line.includes('%API_BASE%'))
    .join('\n')
    .replaceAll('%API_BASE%', apiBase?.replace(/\/+$/, '') ?? '')
  return {
    name: 'llms-txt',
    configureServer(server) {
      server.middlewares.use('/llms.txt', (_request, response) => {
        response.setHeader('Content-Type', 'text/plain; charset=utf-8')
        response.end(render())
      })
    },
    generateBundle() {
      this.emitFile({ type: 'asset', fileName: 'llms.txt', source: render() })
    },
  }
}

export default defineConfig(({ mode }) => ({
  plugins: [react(), llmsTxt(loadEnv(mode, process.cwd(), 'VITE_').VITE_FMO_API_BASE_URL || undefined)],
}))
