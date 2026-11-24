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
    <span className="ml-auto font-mono text-xs text-manifold-ink">
      {children}
    </span>
  );
}

function SidebarNav({ onNavigate, collapsed = false }: { onNavigate?: () => void; collapsed?: boolean }) {
  return (
    <nav aria-label="Main" className={`flex-1 space-y-6 overflow-y-auto py-4 scrollbar-thin ${collapsed ? 'px-2' : 'px-3'}`}>
      {navGroups().map((group) => (
        <div key={group.label}>
          {collapsed ? (
            // Keeps the groups apart without their labels
            <div className="mx-2 mb-2 border-t border-line" aria-hidden="true" />
          ) : (
            <div className={`meta-label mb-2 px-2 ${
              group.label === 'Manifold' ? 'text-manifold-ink' : 'text-ink-3'
            }`}>
              {group.label}
            </div>
          )}
          <ul className="space-y-0.5">
            {group.items.map((item) => (
              <li key={item.to}>
                <NavLink
                  to={item.to}
                  end={item.end}
                  onClick={onNavigate}
                  title={collapsed ? item.label : undefined}
                  aria-label={collapsed ? item.label : undefined}
                  className={({ isActive }) =>
                    `group flex h-8 items-center gap-2.5 rounded-lg text-sm transition-colors ${collapsed ? 'justify-center px-0' : 'px-2'} ${
                      isActive
                        ? `bg-surface-3 font-medium text-ink ${group.label === 'Manifold' ? 'shadow-[inset_2px_0_0_var(--fmo-manifold)]' : 'shadow-[inset_2px_0_0_var(--fmo-accent)]'}`
                        : 'text-ink-2 hover:bg-surface-2 hover:text-ink'
                    }`
                  }
                >
                  <Icon name={item.icon} className="size-4 text-ink-3 group-hover:text-ink-2" />
                  {!collapsed && <span className="truncate">{item.label}</span>}
                  {!collapsed && item.badge}
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

function SidebarFooter({ theme, onToggleTheme, collapsed = false }: { theme: Theme; onToggleTheme: () => void; collapsed?: boolean }) {
  return (
    <div className={`flex items-center gap-1 border-t border-line py-3 ${collapsed ? 'flex-col px-2' : 'px-3'}`}>
      <button
        type="button"
        onClick={onToggleTheme}
        title={collapsed ? `${THEME_LABEL[theme]} theme` : undefined}
        className={`inline-flex h-8 items-center gap-2 rounded-lg text-xs text-ink-2 hover:bg-surface-2 hover:text-ink ${
          collapsed ? 'size-8 justify-center' : 'flex-1 px-2'
        }`}
        aria-label={`Theme: ${THEME_LABEL[theme]}. Switch theme`}
      >
        <Icon name={THEME_ICON[theme]} className="size-4" />
        {!collapsed && `${THEME_LABEL[theme]} theme`}
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
        <span className="block text-sm font-semibold text-ink">Fedimint Observer</span>
        <span className="block text-xs text-ink-3">Federation observability</span>
      </span>
    </Link>
  );
}

const SIDEBAR_KEY = 'fmo-sidebar-collapsed';

/** The desktop sidebar's collapsed state, remembered per browser when storage allows */
function useSidebarCollapsed(): [boolean, () => void] {
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return window.localStorage.getItem(SIDEBAR_KEY) === '1';
    } catch {
      return false;
    }
  });
  const toggle = () =>
    setCollapsed((was) => {
      try {
        window.localStorage.setItem(SIDEBAR_KEY, was ? '0' : '1');
      } catch {
        // Storage blocked: the choice lasts for this page only
      }
      return !was;
    });
  return [collapsed, toggle];
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
  const [collapsed, toggleCollapsed] = useSidebarCollapsed();
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
      <aside
        id="desktop-sidebar"
        className={`fixed inset-y-0 left-0 z-30 hidden flex-col border-r border-line bg-surface lg:flex ${
          collapsed ? 'w-16' : 'w-60'
        }`}
      >
        <div className={`flex h-14 items-center border-b border-line ${collapsed ? 'justify-center px-2' : 'justify-between pl-4 pr-2'}`}>
          {!collapsed && <Brand />}
          <button
            type="button"
            onClick={toggleCollapsed}
            aria-expanded={!collapsed}
            aria-controls="desktop-sidebar"
            aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            className="inline-flex size-8 items-center justify-center rounded-lg text-ink-3 hover:bg-surface-2 hover:text-ink"
          >
            <Icon name="sidebar" className="size-4" />
          </button>
        </div>
        <SidebarNav collapsed={collapsed} />
        <SidebarFooter theme={theme} onToggleTheme={onToggleTheme} collapsed={collapsed} />
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
          <div className="absolute inset-0 bg-black/40" />
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

      <div className={collapsed ? 'lg:pl-16' : 'lg:pl-60'}>
        <main id="main" tabIndex={-1} className="mx-auto w-full max-w-[1440px] px-4 pt-6 pb-12 outline-none sm:px-6 sm:pt-8 lg:px-8">
          {children}
        </main>
      </div>
    </div>
  );
}
