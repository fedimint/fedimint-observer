import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { readStorage, writeStorage } from '../utils/storage';

export type Theme = 'light' | 'dark' | 'auto';

type ThemeContextValue = {
  theme: Theme;
  toggleTheme: () => void;
};

const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);

const prefersDarkMode = () => typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches;

const getStoredTheme = (): Theme | null => {
  if (typeof window === 'undefined') {
    return null;
  }
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

const applyThemeClass = (theme: Theme) => {
  if (typeof document === 'undefined') {
    return;
  }
  const root = document.documentElement;
  // For 'auto', follow the system preference
  const effectiveTheme = theme === 'auto' ? (prefersDarkMode() ? 'dark' : 'light') : theme;
  if (effectiveTheme === 'dark') {
    root.classList.add('dark');
  } else {
    root.classList.remove('dark');
  }
};

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(getInitialTheme);

  // Apply theme class whenever theme changes
  useEffect(() => {
    applyThemeClass(theme);
  }, [theme]);

  // Listen for storage changes from other tabs/windows
  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }
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

  // Listen for system preference changes and apply them when in 'auto' mode
  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) {
      return;
    }
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const handler = () => {
      // Re-apply theme to pick up system preference changes when in auto mode
      if (theme === 'auto') {
        applyThemeClass('auto');
      }
    };
    media.addEventListener('change', handler);
    return () => media.removeEventListener('change', handler);
  }, [theme]);

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
      if (typeof window !== 'undefined') {
        writeStorage('theme', newTheme === 'auto' ? null : newTheme);
      }
      return newTheme;
    });
  }, []);

  const value = useMemo(() => ({ theme, toggleTheme }), [theme, toggleTheme]);

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
