import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react';
import { readStorage, writeStorage } from '../utils/storage';

export type Theme = 'light' | 'dark' | 'auto';

type ThemeContextValue = {
  theme: Theme;
  isDark: boolean;
  toggleTheme: () => void;
};

const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);

const DARK_QUERY = '(prefers-color-scheme: dark)';

const subscribeToColorScheme = (onChange: () => void) => {
  const media = window.matchMedia(DARK_QUERY);
  media.addEventListener('change', onChange);
  return () => media.removeEventListener('change', onChange);
};

const prefersDarkMode = () => window.matchMedia(DARK_QUERY).matches;

const getStoredTheme = (): Theme | null => {
  const stored = readStorage('theme');
  if (stored === 'light' || stored === 'dark' || stored === 'auto') {
    return stored;
  }
  return null;
};

const getInitialTheme = (): Theme => {
  const stored = getStoredTheme();
  if (stored) {
    return stored;
  }
  return 'auto';
};

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(getInitialTheme);
  const prefersDark = useSyncExternalStore(subscribeToColorScheme, prefersDarkMode);
  const isDark = theme === 'dark' || (theme === 'auto' && prefersDark);

  const firstApply = useRef(true);
  useEffect(() => {
    const root = document.documentElement;
    // A switch cross-fades colours briefly; the first paint and reduced motion do not
    const fade = !firstApply.current && !window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    firstApply.current = false;
    if (fade) root.classList.add('theme-switching');
    root.classList.toggle('dark', isDark);
    if (!fade) return;
    const timer = setTimeout(() => root.classList.remove('theme-switching'), 260);
    return () => {
      clearTimeout(timer);
      root.classList.remove('theme-switching');
    };
  }, [isDark]);

  useEffect(() => {
    const handleStorage = (event: StorageEvent) => {
      if (event.key === 'theme' && (event.newValue === 'light' || event.newValue === 'dark' || event.newValue === 'auto')) {
        setTheme(event.newValue);
      } else if (event.key === 'theme' && event.newValue === null) {
        setTheme('auto');
      }
    };
    window.addEventListener('storage', handleStorage);
    return () => window.removeEventListener('storage', handleStorage);
  }, []);

  const toggleTheme = useCallback(() => {
    setTheme(prev => {
      let newTheme: Theme;
      if (prev === 'light') {
        newTheme = 'dark';
      } else if (prev === 'dark') {
        newTheme = 'auto';
      } else {
        newTheme = 'light';
      }

      writeStorage('theme', newTheme === 'auto' ? null : newTheme);
      return newTheme;
    });
  }, []);

  const value = useMemo(() => ({ theme, isDark, toggleTheme }), [theme, isDark, toggleTheme]);

  return (
    <ThemeContext.Provider value={value}>
      {children}
    </ThemeContext.Provider>
  );
}

// eslint-disable-next-line react-refresh/only-export-components
export function useTheme() {
  const context = useContext(ThemeContext);
  if (!context) {
    throw new Error('useTheme must be used within a ThemeProvider');
  }
  return context;
}
