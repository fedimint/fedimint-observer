import { lazy, Suspense, type ComponentType } from 'react';
import { BrowserRouter as Router, Routes, Route, useLocation, useParams } from 'react-router-dom';
import { NavBar } from './components/NavBar';
import { ErrorBoundary, ReloadMessage } from './components/ErrorBoundary';
import { Home } from './pages/Home';
import { useTheme } from './hooks/useTheme';

const Nostr = lazy(() => import('./pages/Nostr').then((module) => ({ default: module.Nostr })));
const FederationDetail = lazy(() => import('./pages/FederationDetail').then((module) => ({ default: module.FederationDetail })));
const FederationGateways = lazy(() => import('./pages/FederationGateways').then((module) => ({ default: module.FederationGateways })));
const FederationGatewaysV2 = lazy(() => import('./pages/FederationGatewaysV2').then((module) => ({ default: module.FederationGatewaysV2 })));

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
      <Suspense fallback={<div role="status" className="min-h-[60vh] py-6 text-sm text-gray-600 dark:text-gray-300">Loading page…</div>}>
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/nostr" element={<Nostr />} />
          <Route path="/federations/:id" element={<PerFederation page={FederationDetail} />} />
          <Route path="/federations/:id/gateways/v2" element={<PerFederation page={FederationGatewaysV2} />} />
          <Route path="/federations/:id/gateways" element={<PerFederation page={FederationGateways} />} />
          <Route path="*" element={<div className="p-4 text-gray-900 dark:text-white">Page not found</div>} />
        </Routes>
      </Suspense>
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
