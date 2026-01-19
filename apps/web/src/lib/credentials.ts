/**
 * Secure credential storage using Tauri Stronghold.
 * Only available in Tauri desktop app, not in WASM.
 */

import { Client, Stronghold } from '@tauri-apps/plugin-stronghold';
import { appDataDir } from '@tauri-apps/api/path';

let strongholdInstance: Stronghold | null = null;
let clientInstance: Client | null = null;
let initPromise: Promise<boolean> | null = null;

const VAULT_FILE = 'diaryx.hold';
const CLIENT_NAME = 'diaryx-credentials';

// App-derived password - this provides encryption without user prompt
// The actual security comes from the OS-level file protection
const APP_DERIVED_PASSWORD = 'diaryx-vault-key-v1';

/**
 * Initialize Stronghold with the app-derived password.
 * Called automatically on first credential access.
 */
async function initCredentialStoreInternal(): Promise<boolean> {
  try {
    console.log('[Credentials] Getting app data dir...');
    const dataDir = await appDataDir();
    const vaultPath = `${dataDir}/${VAULT_FILE}`;
    console.log('[Credentials] Loading Stronghold from:', vaultPath);

    strongholdInstance = await Stronghold.load(vaultPath, APP_DERIVED_PASSWORD);
    console.log('[Credentials] Stronghold loaded, getting client...');

    try {
      clientInstance = await strongholdInstance.loadClient(CLIENT_NAME);
      console.log('[Credentials] Client loaded');
    } catch {
      console.log('[Credentials] Creating new client...');
      clientInstance = await strongholdInstance.createClient(CLIENT_NAME);
      console.log('[Credentials] Client created');
    }

    return true;
  } catch (e) {
    console.error('[Credentials] Failed to init credential store:', e);
    return false;
  }
}

/**
 * Ensure credential store is initialized (auto-init on first use).
 */
async function ensureInitialized(): Promise<void> {
  if (strongholdInstance && clientInstance) return;

  if (!initPromise) {
    initPromise = initCredentialStoreInternal();
  }

  const success = await initPromise;
  if (!success) {
    throw new Error('Failed to initialize credential store');
  }
}

/**
 * Initialize Stronghold with a custom password (legacy API).
 * @deprecated Use the auto-init functions instead
 */
export async function initCredentialStore(password: string): Promise<boolean> {
  try {
    const dataDir = await appDataDir();
    const vaultPath = `${dataDir}/${VAULT_FILE}`;

    strongholdInstance = await Stronghold.load(vaultPath, password);

    try {
      clientInstance = await strongholdInstance.loadClient(CLIENT_NAME);
    } catch {
      clientInstance = await strongholdInstance.createClient(CLIENT_NAME);
    }

    return true;
  } catch (e) {
    console.error('Failed to init credential store:', e);
    return false;
  }
}

/**
 * Check if credential store is initialized.
 */
export function isCredentialStoreReady(): boolean {
  return strongholdInstance !== null && clientInstance !== null;
}

/**
 * Store a credential securely.
 */
export async function storeCredential(key: string, value: string): Promise<void> {
  await ensureInitialized();
  if (!clientInstance || !strongholdInstance) {
    throw new Error('Credential store not initialized.');
  }

  const store = clientInstance.getStore();
  const data = Array.from(new TextEncoder().encode(value));
  await store.insert(key, data);
  await strongholdInstance.save();
}

/**
 * Retrieve a credential.
 */
export async function getCredential(key: string): Promise<string | null> {
  await ensureInitialized();
  if (!clientInstance) {
    throw new Error('Credential store not initialized.');
  }

  const store = clientInstance.getStore();
  try {
    const data = await store.get(key);
    if (!data) return null;
    return new TextDecoder().decode(new Uint8Array(data));
  } catch {
    return null;
  }
}

/**
 * Remove a credential.
 */
export async function removeCredential(key: string): Promise<void> {
  await ensureInitialized();
  if (!clientInstance || !strongholdInstance) {
    throw new Error('Credential store not initialized.');
  }

  const store = clientInstance.getStore();
  await store.remove(key);
  await strongholdInstance.save();
}

// S3 specific helpers - using localStorage for reliability
const S3_ACCESS_KEY = 'diaryx_s3_access_key';
const S3_SECRET_KEY = 'diaryx_s3_secret_key';
const S3_CONFIG = 'diaryx_s3_config';

export interface S3Config {
  name: string;
  bucket: string;
  region: string;
  prefix?: string;
  endpoint?: string;
  access_key: string;
  secret_key: string;
}

/**
 * Store S3 credentials (access key and secret key only).
 */
export async function storeS3Credentials(accessKey: string, secretKey: string): Promise<void> {
  localStorage.setItem(S3_ACCESS_KEY, accessKey);
  localStorage.setItem(S3_SECRET_KEY, secretKey);
}

/**
 * Get S3 credentials only.
 */
export async function getS3Credentials(): Promise<{ accessKey: string; secretKey: string } | null> {
  const accessKey = localStorage.getItem(S3_ACCESS_KEY);
  const secretKey = localStorage.getItem(S3_SECRET_KEY);

  if (!accessKey || !secretKey) return null;
  return { accessKey, secretKey };
}

/**
 * Store full S3 configuration (including credentials).
 */
export async function storeS3Config(config: S3Config): Promise<void> {
  // Store config (without secrets) as JSON
  const configWithoutSecrets = {
    name: config.name,
    bucket: config.bucket,
    region: config.region,
    prefix: config.prefix,
    endpoint: config.endpoint,
  };
  localStorage.setItem(S3_CONFIG, JSON.stringify(configWithoutSecrets));

  // Store secrets separately
  localStorage.setItem(S3_ACCESS_KEY, config.access_key);
  localStorage.setItem(S3_SECRET_KEY, config.secret_key);
}

/**
 * Get full S3 configuration.
 */
export async function getS3Config(): Promise<S3Config | null> {
  try {
    const configJson = localStorage.getItem(S3_CONFIG);
    const accessKey = localStorage.getItem(S3_ACCESS_KEY);
    const secretKey = localStorage.getItem(S3_SECRET_KEY);

    if (!configJson || !accessKey || !secretKey) return null;

    const config = JSON.parse(configJson);
    return {
      ...config,
      access_key: accessKey,
      secret_key: secretKey,
    };
  } catch {
    return null;
  }
}

/**
 * Remove all S3 credentials and config.
 */
export async function removeS3Credentials(): Promise<void> {
  localStorage.removeItem(S3_ACCESS_KEY);
  localStorage.removeItem(S3_SECRET_KEY);
  localStorage.removeItem(S3_CONFIG);
}

// Google Drive specific helpers - using localStorage for reliability
// (Stronghold has async context issues that cause hangs/panics)
const GD_REFRESH_TOKEN = 'diaryx_gd_refresh_token';
const GD_FOLDER_ID = 'diaryx_gd_folder_id';
const GD_CLIENT_ID = 'diaryx_gd_client_id';
const GD_CLIENT_SECRET = 'diaryx_gd_client_secret';

/**
 * Store Google Drive refresh token.
 */
export async function storeGoogleDriveRefreshToken(token: string): Promise<void> {
  localStorage.setItem(GD_REFRESH_TOKEN, token);
}

/**
 * Get Google Drive refresh token.
 */
export async function getGoogleDriveRefreshToken(): Promise<string | null> {
  return localStorage.getItem(GD_REFRESH_TOKEN);
}

/**
 * Store Google Drive folder ID.
 */
export async function storeGoogleDriveFolderId(folderId: string): Promise<void> {
  localStorage.setItem(GD_FOLDER_ID, folderId);
}

/**
 * Get Google Drive folder ID.
 */
export async function getGoogleDriveFolderId(): Promise<string | null> {
  return localStorage.getItem(GD_FOLDER_ID);
}

/**
 * Store Google Client ID and Secret.
 */
export async function storeGoogleDriveCredentials(clientId: string, clientSecret?: string): Promise<void> {
  localStorage.setItem(GD_CLIENT_ID, clientId);
  if (clientSecret) {
    localStorage.setItem(GD_CLIENT_SECRET, clientSecret);
  } else {
    localStorage.removeItem(GD_CLIENT_SECRET);
  }
}

/**
 * Get Google Client ID and Secret.
 */
export async function getGoogleDriveCredentials(): Promise<{ clientId: string | null; clientSecret: string | null }> {
  const clientId = localStorage.getItem(GD_CLIENT_ID);
  const clientSecret = localStorage.getItem(GD_CLIENT_SECRET);
  return { clientId, clientSecret };
}

/**
 * Remove all Google Drive credentials.
 */
export async function removeGoogleDriveCredentials(): Promise<void> {
  localStorage.removeItem(GD_REFRESH_TOKEN);
  localStorage.removeItem(GD_FOLDER_ID);
  localStorage.removeItem(GD_CLIENT_ID);
  localStorage.removeItem(GD_CLIENT_SECRET);
}

// Live Sync specific helpers - using localStorage for reliability
const SYNC_SERVER_URL = 'diaryx_sync_server_url';
const SYNC_ENABLED = 'diaryx_sync_enabled';

export interface SyncConfig {
  serverUrl: string;
  enabled: boolean;
}

/**
 * Store sync configuration.
 */
export async function storeSyncConfig(config: SyncConfig): Promise<void> {
  localStorage.setItem(SYNC_SERVER_URL, config.serverUrl);
  localStorage.setItem(SYNC_ENABLED, config.enabled ? 'true' : 'false');
}

/**
 * Get sync configuration.
 */
export async function getSyncConfig(): Promise<SyncConfig | null> {
  try {
    const serverUrl = localStorage.getItem(SYNC_SERVER_URL);
    const enabled = localStorage.getItem(SYNC_ENABLED);

    if (!serverUrl) return null;

    return {
      serverUrl: serverUrl || '',
      enabled: enabled === 'true',
    };
  } catch {
    return null;
  }
}

/**
 * Remove sync configuration.
 */
export async function removeSyncConfig(): Promise<void> {
  localStorage.removeItem(SYNC_SERVER_URL);
  localStorage.removeItem(SYNC_ENABLED);
}
