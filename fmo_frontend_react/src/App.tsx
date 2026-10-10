import type { ComponentType } from 'react';
import { BrowserRouter as Router, Routes, Route, useLocation, useParams } from 'react-router-dom';
import { NavBar } from './components/NavBar';
import { ErrorBoundary, ReloadMessage } from './components/ErrorBoundary';
import { Home } from './pages/Home';
import { Nostr } from './pages/Nostr';
import { FederationDetail } from './pages/FederationDetail';
import { FederationGateways } from './pages/FederationGateways';
import { useTheme } from './hooks/useTheme';

// The router reuses a page when only :id changes; remount it so it never shows
// the previous federation's data under the new URL
function PerFederation({ page: Page }: { page: ComponentType }) {
  const { id } = useParams<{ id: string }>();
  return <Page key={id} />;
}

function AppRoutes() {
  const { pathname } = useLocation();
  return (
    <ErrorBoundary key={pathname} fallback={<ReloadMessage message="Something went wrong while showing this page." />}>
      <Routes>
        <Route path="/" element={<Home />} />
        <Route path="/nostr" element={<Nostr />} />
        <Route path="/federations/:id" element={<PerFederation page={FederationDetail} />} />
        <Route path="/federations/:id/gateways" element={<PerFederation page={FederationGateways} />} />
        <Route path="*" element={<div className="p-4 text-gray-900 dark:text-white">Page not found</div>} />
      </Routes>
    </ErrorBoundary>
  );
}

function App() {
  const { theme, toggleTheme } = useTheme();

  return (
    <Router>
      <main className="container mx-auto max-w-6xl px-4 min-h-screen pb-4">
        <NavBar theme={theme} onToggleTheme={toggleTheme} />
        <AppRoutes />
      </main>
    </Router>
  );
}

export default App;
