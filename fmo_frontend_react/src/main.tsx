import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { ThemeProvider } from './hooks/useTheme'
import { prefetchFederationRoute } from './services/api'

const CHUNK_RELOAD_KEY = 'chunkReloadBuild'
window.addEventListener('vite:preloadError', (event) => {
  try {
    if (sessionStorage.getItem(CHUNK_RELOAD_KEY) === import.meta.url) return // let the error boundary show
    sessionStorage.setItem(CHUNK_RELOAD_KEY, import.meta.url)
    event.preventDefault()
    window.location.reload()
  } catch {
    // The error boundary handles failures without a storage-backed loop guard.
  }
})

try {
  prefetchFederationRoute(window.location.pathname, window.location.search)
} catch (error) {
  console.warn('Could not prefetch this page:', error)
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </StrictMode>,
)
