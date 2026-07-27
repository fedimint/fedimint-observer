import { BrowserRouter as Router, Routes, Route } from 'react-router-dom';
import { NavBar } from './components/NavBar';
import { Home } from './pages/Home';
import { Nostr } from './pages/Nostr';
import { FederationDetail } from './pages/FederationDetail';
import { FederationGateways } from './pages/FederationGateways';
import { useTheme } from './hooks/useTheme';

function App() {
  const { theme, toggleTheme } = useTheme();

  return (
    <Router>
      <main className="container mx-auto max-w-6xl px-4 min-h-screen pb-4">
        <NavBar theme={theme} onToggleTheme={toggleTheme} />
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/nostr" element={<Nostr />} />
          <Route path="/federations/:id" element={<FederationDetail />} />
          <Route path="/federations/:id/gateways" element={<FederationGateways />} />
          <Route path="*" element={<div className="p-4 text-gray-900 dark:text-white">Page not found</div>} />
        </Routes>
      </main>
    </Router>
  );
}

export default App;
