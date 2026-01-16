/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Dark theme optimized for video editing environments
        background: '#1a1a1a',
        surface: '#242424',
        'surface-hover': '#2d2d2d',
        'surface-active': '#363636',
        border: '#3d3d3d',
        'border-focus': '#5a5a5a',
        text: {
          primary: '#e4e4e4',
          secondary: '#a0a0a0',
          muted: '#666666',
        },
        accent: {
          blue: '#4a9eff',
          'blue-hover': '#6bb3ff',
          green: '#4ade80',
          yellow: '#fbbf24',
          red: '#f87171',
          purple: '#a78bfa',
        },
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', '-apple-system', 'sans-serif'],
        mono: ['JetBrains Mono', 'Consolas', 'monospace'],
      },
      animation: {
        'progress-pulse': 'progress-pulse 2s ease-in-out infinite',
      },
      keyframes: {
        'progress-pulse': {
          '0%, 100%': { opacity: 1 },
          '50%': { opacity: 0.6 },
        },
      },
    },
  },
  plugins: [],
}
