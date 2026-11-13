import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Link, NavLink, useLocation } from 'react-router-dom';
import type { Theme } from '../../hooks/useTheme';
import { Icon, type IconName } from '../ui/Icon';

interface NavItem {
  to: string;
  label: string;
  icon: IconName;
  end?: boolean;
  badge?: ReactNode;
}

interface NavGroup {
  label: string;
  items: NavItem[];
}

function navGroups(): NavGroup[] {
  const groups: NavGroup[] = [
    {
      label: 'Explore',
      items: [
        { to: '/', label: 'Federations', icon: 'federations', end: true },
        { to: '/nostr', label: 'Nostr announcements', icon: 'nostr' },
      ],
    },
  ];
  groups.push({
    label: 'Manifold',
    items: [
      {
        to: '/manifold/guardians',
        label: 'Guardian directory',
        icon: 'directory',
      },
      { to: '/manifold/flip', label: 'FLIP providers', icon: 'liquidity', badge: <NavTag>Preview</NavTag> },
    ],
  });
  return groups;
}

function NavTag({ children }: { children: ReactNode }) {
  return (
    <span className="ml-auto rounded bg-manifold-soft px-1.5 py-px text-[0.6rem] font-semibold uppercase tracking-wide text-manifold-ink">
      {children}
    </span>
  );
}

function SidebarNav({ onNavigate }: { onNavigate?: () => void }) {
  return (
    <nav aria-label="Main" className="flex-1 space-y-5 overflow-y-auto px-3 py-4 scrollbar-thin">
      {navGroups().map((group) => (
        <div key={group.label}>
          <div className={`mb-1.5 px-2 text-[0.65rem] font-semibold uppercase tracking-[0.1em] ${
            group.label === 'Manifold' ? 'text-manifold-ink' : 'text-ink-3'
          }`}>
            {group.label}
          </div>
          <ul className="space-y-0.5">
            {group.items.map((item) => (
              <li key={item.to}>
                <NavLink
                  to={item.to}
                  end={item.end}
                  onClick={onNavigate}
                  className={({ isActive }) =>
                    `group flex items-center gap-2.5 rounded-lg px-2 py-1.5 text-sm transition-colors ${
                      isActive
                        ? `bg-surface-3 font-medium text-ink ${group.label === 'Manifold' ? 'shadow-[inset_2px_0_0_var(--fmo-manifold)]' : 'shadow-[inset_2px_0_0_var(--fmo-accent)]'}`
                        : 'text-ink-2 hover:bg-surface-2 hover:text-ink'
                    }`
                  }
                >
                  <Icon name={item.icon} className="size-4 text-ink-3 group-hover:text-ink-2" />
                  <span className="truncate">{item.label}</span>
                  {item.badge}
                </NavLink>
              </li>
            ))}
          </ul>
        </div>
      ))}
    </nav>
  );
}

const THEME_LABEL: Record<Theme, string> = { light: 'Light', dark: 'Dark', auto: 'System' };
const THEME_ICON: Record<Theme, IconName> = { light: 'sun', dark: 'moon', auto: 'monitor' };

function SidebarFooter({ theme, onToggleTheme }: { theme: Theme; onToggleTheme: () => void }) {
  return (
    <div className="flex items-center gap-1 border-t border-line px-3 py-3">
      <button
        type="button"
        onClick={onToggleTheme}
        className="inline-flex flex-1 items-center gap-2 rounded-lg px-2 py-1.5 text-xs text-ink-2 hover:bg-surface-2 hover:text-ink"
        aria-label={`Theme: ${THEME_LABEL[theme]}. Switch theme`}
      >
        <Icon name={THEME_ICON[theme]} className="size-4" />
        {THEME_LABEL[theme]} theme
      </button>
      <a
        href="https://github.com/fedimint/fedimint-observer"
        target="_blank"
        rel="noreferrer"
        aria-label="Fedimint Observer source on GitHub"
        className="inline-flex size-8 items-center justify-center rounded-lg text-ink-3 hover:bg-surface-2 hover:text-ink"
      >
        <Icon name="github" className="size-4" />
      </a>
    </div>
  );
}

function Brand() {
  return (
    <Link to="/" className="flex items-center gap-2.5">
      <img src="/fedimint.png" width="128" height="125" className="h-7 w-auto" alt="" />
      <span className="leading-tight">
        <span className="block text-sm font-semibold tracking-tight text-ink">Fedimint Observer</span>
        <span className="block text-[0.65rem] text-ink-3">Federation observability</span>
      </span>
    </Link>
  );
}

interface AppShellProps {
  theme: Theme;
  onToggleTheme: () => void;
  children: ReactNode;
}

/** Persistent sidebar on large screens; below that, a top bar whose menu button opens the navigation as a dropdown under it */
export function AppShell({ theme, onToggleTheme, children }: AppShellProps) {
  const { pathname } = useLocation();
  const [menuOpen, setMenuOpen] = useState(false);
  const menuButton = useRef<HTMLButtonElement>(null);
  const drawer = useRef<HTMLDivElement>(null);

  // Close the drawer on navigation and with Escape, returning focus to its button
  useEffect(() => setMenuOpen(false), [pathname]);
  useEffect(() => {
    if (!menuOpen) return;
    drawer.current?.querySelector<HTMLElement>('a, button')?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setMenuOpen(false);
        menuButton.current?.focus();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [menuOpen]);

  return (
    <div className="min-h-screen bg-canvas">
      <a href="#main" className="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[60] focus:rounded-lg focus:bg-surface focus:px-3 focus:py-2 focus:text-sm">
        Skip to content
      </a>

      {/* Desktop sidebar */}
      <aside className="fixed inset-y-0 left-0 z-30 hidden w-60 flex-col border-r border-line bg-surface lg:flex">
        <div className="flex h-14 items-center border-b border-line px-4">
          <Brand />
        </div>
        <SidebarNav />
        <SidebarFooter theme={theme} onToggleTheme={onToggleTheme} />
      </aside>

      {/* Mobile and tablet top bar */}
      <div className="sticky top-0 z-30 flex h-14 items-center justify-between border-b border-line bg-surface/95 px-4 backdrop-blur lg:hidden">
        <Brand />
        <button
          ref={menuButton}
          type="button"
          onClick={() => setMenuOpen((open) => !open)}
          aria-expanded={menuOpen}
          aria-controls="mobile-navigation"
          aria-label={menuOpen ? 'Close navigation' : 'Open navigation'}
          className="inline-flex size-9 items-center justify-center rounded-lg text-ink-2 hover:bg-surface-3"
        >
          <Icon name={menuOpen ? 'close' : 'menu'} className="size-5" />
        </button>
      </div>
      {menuOpen && (
        <div className="fixed inset-0 top-14 z-30 lg:hidden" role="presentation" onMouseDown={() => setMenuOpen(false)}>
          <div className="absolute inset-0 bg-gray-950/40" />
          {/* A dropdown under the menu button, right-aligned with it */}
          <div
            id="mobile-navigation"
            ref={drawer}
            onMouseDown={(event) => event.stopPropagation()}
            className="absolute top-2 right-4 flex max-h-[calc(100%-1rem)] w-72 max-w-[calc(100vw-2rem)] origin-top-right flex-col overflow-hidden rounded-xl border border-line bg-surface shadow-xl"
          >
            <SidebarNav onNavigate={() => setMenuOpen(false)} />
            <SidebarFooter theme={theme} onToggleTheme={onToggleTheme} />
          </div>
        </div>
      )}

      <div className="lg:pl-60">
        <main id="main" tabIndex={-1} className="mx-auto w-full max-w-7xl px-4 pt-5 pb-10 outline-none sm:px-6 sm:pt-7 lg:px-8">
          {children}
        </main>
      </div>
    </div>
  );
}
