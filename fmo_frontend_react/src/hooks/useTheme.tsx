import { createContext, useCallback, useContext, useEffect, useMemo, useState, useSyncExternalStore, type ReactNode } from 'react';
import { readStorage, writeStorage } from '../utils/storage';

export type Theme = 'light' | 'dark' | 'auto';

type ThemeContextValue = {
  theme: Theme;
  // Whether the page is dark, with 'auto' following the system; for charts and
  // anything else the dark: classes can't reach
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
  // Default to 'auto' if no preference is stored
  return 'auto';
};

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(getInitialTheme);
  // Re-renders when the system switches between light and dark, for the 'auto' theme
  const prefersDark = useSyncExternalStore(subscribeToColorScheme, prefersDarkMode);
  const isDark = theme === 'dark' || (theme === 'auto' && prefersDark);

  // Apply theme class whenever the effective theme changes
  useEffect(() => {
    document.documentElement.classList.toggle('dark', isDark);
  }, [isDark]);

  // Listen for storage changes from other tabs/windows
  useEffect(() => {
    const handleStorage = (event: StorageEvent) => {
      if (event.key === 'theme' && (event.newValue === 'light' || event.newValue === 'dark' || event.newValue === 'auto')) {
        setTheme(event.newValue);
      } else if (event.key === 'theme' && event.newValue === null) {
        // Theme was cleared, default to auto
        setTheme('auto');
      }
    };
    window.addEventListener('storage', handleStorage);
    return () => window.removeEventListener('storage', handleStorage);
  }, []);

  const toggleTheme = useCallback(() => {
    setTheme(prev => {
      // Cycle through: light -> dark -> auto -> light
      let newTheme: Theme;
      if (prev === 'light') {
        newTheme = 'dark';
      } else if (prev === 'dark') {
        newTheme = 'auto';
      } else {
        newTheme = 'light';
      }
      
      // Save to localStorage, or clear it for auto mode
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
