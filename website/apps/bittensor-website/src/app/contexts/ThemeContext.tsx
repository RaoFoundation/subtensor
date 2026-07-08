import React, {createContext, useContext, useState, useEffect, ReactNode} from 'react';

type Theme = 'light' | 'dark';

interface ThemeContextType {
  theme: Theme;
  toggleTheme: () => void;
}

const ThemeContext = createContext<ThemeContextType | undefined>(undefined);

export const ThemeProvider = ({children}: {children: ReactNode}) => {
  const [theme, setTheme] = useState<Theme>('light');

  useEffect(() => {
    const savedTheme = localStorage.getItem('theme') as Theme;
    if (savedTheme) setTheme(savedTheme);
  }, []);

  const toggleTheme = () => {
    const newTheme = theme === 'light' ? 'dark' : 'light';
    setTheme(newTheme);
    localStorage.setItem('theme', newTheme);
  };

  return <ThemeContext.Provider value={{theme, toggleTheme}}>{children}</ThemeContext.Provider>;
};

export const useTheme = () => {
  const context = useContext(ThemeContext);
  if (!context) throw new Error('useTheme must be used within a ThemeProvider');
  return context;
};

type HexColor = `#${string}`;

type AvailableColors = 'textPrimary' | 'textSecondary' | 'bgPrimary' | 'contrast50' | 'contrast100';
type ThemeColors = Record<AvailableColors, HexColor>;

enum Colors {
  white = '#ffffff',
  grey100 = '#f8f9fa',
  grey200 = '#f1f3f4',
  grey300 = '#dbdde1',
  grey400 = '#bdc1c6',
  grey500 = '#5f6368',
  grey600 = '#2e3134',
  grey700 = '#282a2d',
  grey800 = '#17181b',
  grey900 = '#0e1013',
  black = '#000000',
}

const lightColors: ThemeColors = {
  textPrimary: Colors.grey900,
  textSecondary: Colors.grey500,
  bgPrimary: Colors.white,
  contrast50: Colors.grey100,
  contrast100: Colors.grey200,
};

const darkColors: ThemeColors = {
  textPrimary: Colors.grey100,
  textSecondary: Colors.grey500,
  bgPrimary: Colors.black,
  contrast50: Colors.grey900,
  contrast100: Colors.grey800,
};

export const useColors = () => {
  const {theme} = useTheme();

  return theme === 'light' ? lightColors : darkColors;
};
