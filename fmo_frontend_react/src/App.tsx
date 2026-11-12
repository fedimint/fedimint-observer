import { lazy, Suspense, type ComponentType } from 'react';
import { BrowserRouter as Router, Routes, Route, useLocation, useParams } from 'react-router-dom';
import { AppShell } from './components/layout/AppShell';
import { ErrorBoundary, ReloadMessage } from './components/ui/ErrorBoundary';
import { Home } from './pages/Home';
import { useTheme } from './hooks/useTheme';

const Nostr = lazy(() => import('./pages/Nostr').then((module) => ({ default: module.Nostr })));
const FederationDetail = lazy(() => import('./pages/FederationDetail').then((module) => ({ default: module.FederationDetail })));
const FederationSection = lazy(() => import('./pages/FederationSection').then((module) => ({ default: module.FederationSection })));
const FederationActivity = () => <FederationSection section="activity" />;
const FederationUtxos = () => <FederationSection section="utxos" />;
const FederationAssets = () => <FederationSection section="assets" />;
const FederationConfig = () => <FederationSection section="config" />;
const FederationGateways = lazy(() => import('./pages/FederationGateways').then((module) => ({ default: module.FederationGateways })));
const FederationGatewaysV1 = () => <FederationGateways version={1} />;
const FederationGatewaysV2 = () => <FederationGateways version={2} />;
const FederationGuardians = lazy(() => import('./pages/FederationGuardians').then((module) => ({ default: module.FederationGuardians })));
const FederationReliability = lazy(() => import('./pages/FederationReliability').then((module) => ({ default: module.FederationReliability })));
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

function AppRoutes() {
  const { pathname } = useLocation();
  return (
    <ErrorBoundary key={pathname} fallback={<ReloadMessage message="Something went wrong while showing this page." />}>
      <Suspense fallback={<div role="status" className="min-h-[60vh] py-6 text-sm text-ink-3">Loading page…</div>}>
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/nostr" element={<Nostr />} />
          <Route path="/federations/:id" element={<PerFederation page={FederationDetail} />} />
          <Route path="/federations/:id/gateways/v2" element={<PerFederation page={FederationGatewaysV2} />} />
          <Route path="/federations/:id/gateways" element={<PerFederation page={FederationGatewaysV1} />} />
          <Route path="/federations/:id/guardians" element={<PerFederation page={FederationGuardians} />} />
          <Route path="/federations/:id/reliability" element={<PerFederation page={FederationReliability} />} />
          <Route path="/federations/:id/activity" element={<PerFederation page={FederationActivity} />} />
          <Route path="/federations/:id/utxos" element={<PerFederation page={FederationUtxos} />} />
          <Route path="/federations/:id/assets" element={<PerFederation page={FederationAssets} />} />
          <Route path="/federations/:id/config" element={<PerFederation page={FederationConfig} />} />
          <Route path="/manifold/guardians" element={<ManifoldDirectory />} />
          <Route path="/manifold/guardians/:pubkey" element={<PerOperator />} />
          <Route path="/manifold/flip" element={<FlipProviders />} />
          <Route path="*" element={<div className="py-10 text-sm text-ink-2">Page not found</div>} />
        </Routes>
      </Suspense>
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
