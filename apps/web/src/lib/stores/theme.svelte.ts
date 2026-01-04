/**
 * Theme store for managing dark/light mode.
 * Uses Svelte 5 runes for reactive state management.
 * Persists preference to localStorage and respects system preference.
 */

export type ThemeMode = "light" | "dark" | "system";

const STORAGE_KEY = "diaryx-theme";

/**
 * Creates reactive theme state with persistence.
 */
export function createThemeStore() {
  let mode = $state<ThemeMode>("system");
  let resolvedTheme = $state<"light" | "dark">("light");

  // Initialize from localStorage or default to system
  if (typeof window !== "undefined") {
    const stored = localStorage.getItem(STORAGE_KEY) as ThemeMode | null;
    if (stored && ["light", "dark", "system"].includes(stored)) {
      mode = stored;
    }

    // Listen for system preference changes
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    function updateResolvedTheme() {
      if (mode === "system") {
        resolvedTheme = mediaQuery.matches ? "dark" : "light";
      } else {
        resolvedTheme = mode;
      }
      applyTheme(resolvedTheme);
    }

    mediaQuery.addEventListener("change", updateResolvedTheme);

    // Initial resolution
    updateResolvedTheme();
  }

  function applyTheme(theme: "light" | "dark") {
    if (typeof document === "undefined") return;

    const root = document.documentElement;
    if (theme === "dark") {
      root.classList.add("dark");
    } else {
      root.classList.remove("dark");
    }
  }

  function setMode(newMode: ThemeMode) {
    mode = newMode;

    // Persist to localStorage
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(STORAGE_KEY, newMode);
    }

    // Update resolved theme
    if (typeof window !== "undefined") {
      if (newMode === "system") {
        const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
        resolvedTheme = mediaQuery.matches ? "dark" : "light";
      } else {
        resolvedTheme = newMode;
      }
      applyTheme(resolvedTheme);
    }
  }

  function toggle() {
    // Toggle between light and dark (skip system in toggle)
    setMode(resolvedTheme === "dark" ? "light" : "dark");
  }

  return {
    get mode() {
      return mode;
    },
    get resolvedTheme() {
      return resolvedTheme;
    },
    get isDark() {
      return resolvedTheme === "dark";
    },
    setMode,
    toggle,
  };
}

/**
 * Singleton instance for shared theme state across components.
 */
let sharedThemeStore: ReturnType<typeof createThemeStore> | null = null;

export function getThemeStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    return {
      get mode() {
        return "system" as ThemeMode;
      },
      get resolvedTheme() {
        return "light" as const;
      },
      get isDark() {
        return false;
      },
      setMode: () => {},
      toggle: () => {},
    };
  }

  if (!sharedThemeStore) {
    sharedThemeStore = createThemeStore();
  }
  return sharedThemeStore;
}
