import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { ThemeProvider } from './hooks/useTheme'

// A tab opened before a deploy still points at the previous build's chunks, which the
// deploy removed. Reload once to pick up the new build instead of failing the page.
// The guard is per build, so a chunk that keeps failing can never cause a reload loop.
const CHUNK_RELOAD_KEY = 'chunkReloadBuild'
window.addEventListener('vite:preloadError', (event) => {
  try {
    if (sessionStorage.getItem(CHUNK_RELOAD_KEY) === import.meta.url) return // let the error boundary show
    sessionStorage.setItem(CHUNK_RELOAD_KEY, import.meta.url)
    event.preventDefault()
    window.location.reload()
  } catch {
    // Without storage there is no loop guard, so leave it to the error boundary
  }
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </StrictMode>,
)
