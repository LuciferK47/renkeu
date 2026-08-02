/** @type {import('tailwindcss').Config} */
export default {
  // Follow the app's own toggle, not the OS. Without this Tailwind defaults to
  // `media`, so every `dark:` utility tracked prefers-color-scheme and silently
  // ignored the in-app theme switch.
  darkMode: ['selector', '[data-theme="dark"]'],
  content: [
    './index.html',
    './src/**/*.{svelte,js,ts}'
  ],
  theme: {
    extend: {
      fontFamily: {
        sans: ['Outfit', 'system-ui', '-apple-system', 'sans-serif'],
        mono: ['JetBrains Mono', 'ui-monospace', 'SFMono-Regular', 'monospace'],
      },
      colors: {
        border: 'hsl(var(--border))',
        input: 'hsl(var(--input))',
        ring: 'hsl(var(--ring))',
        background: 'hsl(var(--background))',
        foreground: 'hsl(var(--foreground))',
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))',
        },
        secondary: {
          DEFAULT: 'hsl(var(--secondary))',
          foreground: 'hsl(var(--secondary-foreground))',
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive))',
          foreground: 'hsl(var(--destructive-foreground))',
        },
        muted: {
          DEFAULT: 'hsl(var(--muted))',
          foreground: 'hsl(var(--muted-foreground))',
        },
        accent: {
          DEFAULT: 'hsl(var(--accent))',
          foreground: 'hsl(var(--accent-foreground))',
        },
        popover: {
          DEFAULT: 'hsl(var(--popover))',
          foreground: 'hsl(var(--popover-foreground))',
        },
        card: {
          DEFAULT: 'hsl(var(--card))',
          foreground: 'hsl(var(--card-foreground))',
        },
      },
      // One radius scale, shared with the CSS ladder in globals.css. There
      // were two `borderRadius` keys in this object; JavaScript keeps the last,
      // so the earlier one was silently discarded and `rounded` kept resolving
      // to Tailwind's stock 0.25rem.
      borderRadius: {
        DEFAULT: 'var(--radius-xs)',
        sm: 'var(--radius-xs)',
        md: 'var(--radius-s)',
        lg: 'var(--radius-m)',
        full: 'var(--radius-pill)',
      },
    },
  },
  plugins: [],
}
