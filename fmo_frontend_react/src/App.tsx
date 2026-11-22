import { lazy, Suspense, type ComponentType } from 'react';
import { BrowserRouter as Router, Routes, Route, useLocation, useParams } from 'react-router-dom';
import { AppShell } from './components/layout/AppShell';
import { ErrorBoundary, ReloadMessage } from './components/ui/ErrorBoundary';
import { Home } from './pages/Home';
import { useTheme } from './hooks/useTheme';
import {
  FederationDetail,
  FederationGateways,
  FederationGuardians,
  FederationLayout,
  FederationReliability,
  FederationSection,
} from './pages/federationPages';

const Nostr = lazy(() => import('./pages/Nostr').then((module) => ({ default: module.Nostr })));
const ManifoldDirectory = lazy(() => import('./pages/ManifoldDirectory').then((module) => ({ default: module.ManifoldDirectory })));
const ManifoldOperator = lazy(() => import('./pages/ManifoldOperator').then((module) => ({ default: module.ManifoldOperator })));
const FlipProviders = lazy(() => import('./pages/FlipProviders').then((module) => ({ default: module.FlipProviders })));

// Remounted per operator key, like PerFederation
function PerOperator() {
  const { pubkey } = useParams<{ pubkey: string }>();
  return <ManifoldOperator key={pubkey} />;
}

// The router reuses a page when only :id changes; remount it so it never shows
// the previous federation's data under the new URL
function PerFederation({ page: Page }: { page: ComponentType }) {
  const { id } = useParams<{ id: string }>();
  return <Page key={id} />;
}

const NOT_FOUND = <div className="py-10 text-sm text-ink-2">Page not found</div>;

function AppRoutes() {
  const { pathname } = useLocation();
  // A federation's tabs share one boundary, so its header stays while they switch
  const boundary = /^\/federations\/[^/]+/.exec(pathname)?.[0] ?? pathname;
  return (
    <ErrorBoundary key={boundary} fallback={<ReloadMessage message="Something went wrong while showing this page." />}>
      {/* Remounts with the boundary, so each top-level page fades in; a federation's tabs fade inside its layout */}
      <div className="fmo-section-enter">
        <Suspense fallback={<div role="status" className="min-h-[60vh] py-6 text-sm text-ink-3">Loading page…</div>}>
          <Routes>
            <Route path="/" element={<Home />} />
            <Route path="/nostr" element={<Nostr />} />
            <Route path="/federations/:id" element={<PerFederation page={FederationLayout} />}>
              <Route index element={<FederationDetail />} />
              <Route path="guardians" element={<FederationGuardians />} />
              <Route path="reliability" element={<FederationReliability />} />
              <Route path="gateways" element={<FederationGateways version={1} />} />
              <Route path="gateways/v2" element={<FederationGateways version={2} />} />
              <Route path="activity" element={<FederationSection section="activity" />} />
              <Route path="utxos" element={<FederationSection section="utxos" />} />
              <Route path="assets" element={<FederationSection section="assets" />} />
              <Route path="config" element={<FederationSection section="config" />} />
              <Route path="*" element={NOT_FOUND} />
            </Route>
            <Route path="/manifold/guardians" element={<ManifoldDirectory />} />
            <Route path="/manifold/guardians/:pubkey" element={<PerOperator />} />
            <Route path="/manifold/flip" element={<FlipProviders />} />
            <Route path="*" element={NOT_FOUND} />
          </Routes>
        </Suspense>
      </div>
    </ErrorBoundary>
  );
}

function App() {
  const { theme, toggleTheme } = useTheme();

  return (
    <Router>
      <AppShell theme={theme} onToggleTheme={toggleTheme}>
        <AppRoutes />
      </AppShell>
    </Router>
  );
}

export default App;
