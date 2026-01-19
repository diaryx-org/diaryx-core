/**
 * Auth Store - Svelte 5 reactive state for authentication.
 *
 * Manages:
 * - Authentication state (logged in/out)
 * - Session token storage
 * - User info
 * - Auto-connect to sync server when logged in
 */

import { AuthService, createAuthService, type User, type Workspace, type Device, AuthError } from './authService';
import { setAuthToken, setCollaborationServer, setCollaborationWorkspaceId } from '../crdt/collaborationBridge';

// ============================================================================
// Types
// ============================================================================

export interface AuthState {
  isAuthenticated: boolean;
  isLoading: boolean;
  user: User | null;
  workspaces: Workspace[];
  devices: Device[];
  error: string | null;
  serverUrl: string | null;
}

// ============================================================================
// Storage Keys
// ============================================================================

const STORAGE_KEYS = {
  TOKEN: 'diaryx_auth_token',
  SERVER_URL: 'diaryx_sync_server_url',
  USER: 'diaryx_user',
} as const;

// ============================================================================
// State
// ============================================================================

let state = $state<AuthState>({
  isAuthenticated: false,
  isLoading: false,
  user: null,
  workspaces: [],
  devices: [],
  error: null,
  serverUrl: null,
});

let authService: AuthService | null = null;

// ============================================================================
// Getters
// ============================================================================

export function getAuthState(): AuthState {
  return state;
}

export function isAuthenticated(): boolean {
  return state.isAuthenticated;
}

export function getUser(): User | null {
  return state.user;
}

export function getToken(): string | null {
  if (typeof localStorage === 'undefined') return null;
  return localStorage.getItem(STORAGE_KEYS.TOKEN);
}

export function getServerUrl(): string | null {
  return state.serverUrl;
}

export function getDefaultWorkspace(): Workspace | null {
  return state.workspaces.find(w => w.name === 'default') ?? state.workspaces[0] ?? null;
}

// ============================================================================
// Actions
// ============================================================================

/**
 * Initialize auth state from localStorage.
 */
export async function initAuth(): Promise<void> {
  if (typeof localStorage === 'undefined') return;

  const serverUrl = localStorage.getItem(STORAGE_KEYS.SERVER_URL);
  const token = localStorage.getItem(STORAGE_KEYS.TOKEN);
  const savedUser = localStorage.getItem(STORAGE_KEYS.USER);

  if (serverUrl) {
    state.serverUrl = serverUrl;
    authService = createAuthService(serverUrl);
    setCollaborationServer(serverUrl);
  }

  if (token && serverUrl) {
    state.isLoading = true;
    state.error = null;

    // Restore user from localStorage immediately for faster UI
    if (savedUser) {
      try {
        state.user = JSON.parse(savedUser);
        state.isAuthenticated = true;
      } catch {
        // Invalid saved user
      }
    }

    try {
      // Validate token with server
      const me = await authService!.getMe(token);
      state.user = me.user;
      state.workspaces = me.workspaces;
      state.devices = me.devices;
      state.isAuthenticated = true;

      // Update collaboration settings
      setAuthToken(token);
      const defaultWorkspace = me.workspaces.find(w => w.name === 'default') ?? me.workspaces[0];
      if (defaultWorkspace) {
        setCollaborationWorkspaceId(defaultWorkspace.id);
      }

      // Save user for faster restore next time
      localStorage.setItem(STORAGE_KEYS.USER, JSON.stringify(me.user));
    } catch (err) {
      if (err instanceof AuthError && err.statusCode === 401) {
        // Token expired, clear auth state
        await logout();
      } else {
        // Network error - keep user logged in with cached data
        console.warn('[AuthStore] Failed to validate token:', err);
        if (savedUser) {
          state.isAuthenticated = true;
        }
      }
    } finally {
      state.isLoading = false;
    }
  }
}

/**
 * Set the sync server URL.
 */
export function setServerUrl(url: string | null): void {
  state.serverUrl = url;

  if (url) {
    localStorage.setItem(STORAGE_KEYS.SERVER_URL, url);
    authService = createAuthService(url);
    setCollaborationServer(url);
  } else {
    localStorage.removeItem(STORAGE_KEYS.SERVER_URL);
    authService = null;
    setCollaborationServer(null);
  }
}

/**
 * Request a magic link.
 */
export async function requestMagicLink(email: string): Promise<{ success: boolean; devLink?: string }> {
  if (!authService) {
    throw new Error('Server URL not configured');
  }

  state.isLoading = true;
  state.error = null;

  try {
    const response = await authService.requestMagicLink(email);
    return { success: true, devLink: response.dev_link };
  } catch (err) {
    const message = err instanceof Error ? err.message : 'Failed to send magic link';
    state.error = message;
    throw err;
  } finally {
    state.isLoading = false;
  }
}

/**
 * Verify a magic link token and log in.
 */
export async function verifyMagicLink(token: string): Promise<void> {
  if (!authService) {
    throw new Error('Server URL not configured');
  }

  state.isLoading = true;
  state.error = null;

  try {
    // Get device name
    const deviceName = getDeviceName();

    const response = await authService.verifyMagicLink(token, deviceName);

    // Store token
    localStorage.setItem(STORAGE_KEYS.TOKEN, response.token);
    localStorage.setItem(STORAGE_KEYS.USER, JSON.stringify(response.user));

    // Update state
    state.user = response.user;
    state.isAuthenticated = true;

    // Update collaboration settings
    setAuthToken(response.token);

    // Fetch full user info (workspaces, devices)
    await refreshUserInfo();
  } catch (err) {
    const message = err instanceof Error ? err.message : 'Failed to verify magic link';
    state.error = message;
    throw err;
  } finally {
    state.isLoading = false;
  }
}

/**
 * Refresh user info from server.
 */
export async function refreshUserInfo(): Promise<void> {
  const token = getToken();
  if (!authService || !token) return;

  try {
    const me = await authService.getMe(token);
    state.user = me.user;
    state.workspaces = me.workspaces;
    state.devices = me.devices;

    // Update workspace ID
    const defaultWorkspace = me.workspaces.find(w => w.name === 'default') ?? me.workspaces[0];
    if (defaultWorkspace) {
      setCollaborationWorkspaceId(defaultWorkspace.id);
    }
  } catch (err) {
    console.error('[AuthStore] Failed to refresh user info:', err);
  }
}

/**
 * Log out and clear auth state.
 */
export async function logout(): Promise<void> {
  const token = getToken();

  // Clear local state first
  state.isAuthenticated = false;
  state.user = null;
  state.workspaces = [];
  state.devices = [];
  state.error = null;

  localStorage.removeItem(STORAGE_KEYS.TOKEN);
  localStorage.removeItem(STORAGE_KEYS.USER);

  // Clear collaboration settings
  setAuthToken(undefined);
  setCollaborationWorkspaceId(null);

  // Try to logout on server (don't wait for it)
  if (authService && token) {
    authService.logout(token).catch(() => {
      // Ignore logout errors
    });
  }
}

/**
 * Delete a device.
 */
export async function deleteDevice(deviceId: string): Promise<void> {
  const token = getToken();
  if (!authService || !token) return;

  await authService.deleteDevice(token, deviceId);
  await refreshUserInfo();
}

// ============================================================================
// Helpers
// ============================================================================

function getDeviceName(): string {
  if (typeof navigator === 'undefined') return 'Unknown';

  const ua = navigator.userAgent;

  // Check for common browsers/platforms
  if (ua.includes('Chrome')) {
    if (ua.includes('Android')) return 'Chrome (Android)';
    if (ua.includes('iPhone') || ua.includes('iPad')) return 'Chrome (iOS)';
    if (ua.includes('Windows')) return 'Chrome (Windows)';
    if (ua.includes('Mac')) return 'Chrome (Mac)';
    if (ua.includes('Linux')) return 'Chrome (Linux)';
    return 'Chrome';
  }
  if (ua.includes('Firefox')) {
    if (ua.includes('Android')) return 'Firefox (Android)';
    if (ua.includes('Windows')) return 'Firefox (Windows)';
    if (ua.includes('Mac')) return 'Firefox (Mac)';
    if (ua.includes('Linux')) return 'Firefox (Linux)';
    return 'Firefox';
  }
  if (ua.includes('Safari') && !ua.includes('Chrome')) {
    if (ua.includes('iPhone')) return 'Safari (iPhone)';
    if (ua.includes('iPad')) return 'Safari (iPad)';
    if (ua.includes('Mac')) return 'Safari (Mac)';
    return 'Safari';
  }
  if (ua.includes('Tauri')) {
    if (ua.includes('Windows')) return 'Diaryx (Windows)';
    if (ua.includes('Mac')) return 'Diaryx (Mac)';
    if (ua.includes('Linux')) return 'Diaryx (Linux)';
    return 'Diaryx Desktop';
  }

  return 'Web Browser';
}
