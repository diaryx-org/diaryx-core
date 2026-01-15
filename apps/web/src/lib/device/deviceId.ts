/**
 * Device identification service for multi-device sync attribution.
 *
 * Generates and persists unique device IDs and human-readable device names
 * for tracking which device made changes in the history.
 */

const DEVICE_ID_KEY = 'diaryx-device-id';
const DEVICE_NAME_KEY = 'diaryx-device-name';

/**
 * Get or generate a unique device ID.
 * Uses localStorage to persist across sessions.
 */
export function getDeviceId(): string {
  let id = localStorage.getItem(DEVICE_ID_KEY);
  if (!id) {
    id = crypto.randomUUID();
    localStorage.setItem(DEVICE_ID_KEY, id);
  }
  return id;
}

/**
 * Get or generate a human-readable device name.
 * Defaults to browser/OS detection if not set.
 */
export function getDeviceName(): string {
  let name = localStorage.getItem(DEVICE_NAME_KEY);
  if (!name) {
    name = detectDeviceName();
    localStorage.setItem(DEVICE_NAME_KEY, name);
  }
  return name;
}

/**
 * Set a custom device name.
 */
export function setDeviceName(name: string): void {
  localStorage.setItem(DEVICE_NAME_KEY, name);
}

/**
 * Detect a reasonable device name from the browser/OS.
 */
function detectDeviceName(): string {
  const ua = navigator.userAgent;

  // Try to detect platform
  let platform = 'Unknown';
  if (ua.includes('iPhone')) {
    platform = 'iPhone';
  } else if (ua.includes('iPad')) {
    platform = 'iPad';
  } else if (ua.includes('Android')) {
    platform = 'Android';
  } else if (ua.includes('Mac')) {
    platform = 'Mac';
  } else if (ua.includes('Windows')) {
    platform = 'Windows';
  } else if (ua.includes('Linux')) {
    platform = 'Linux';
  }

  // Try to detect browser
  let browser = '';
  if (ua.includes('Firefox')) {
    browser = 'Firefox';
  } else if (ua.includes('Safari') && !ua.includes('Chrome')) {
    browser = 'Safari';
  } else if (ua.includes('Chrome')) {
    browser = 'Chrome';
  } else if (ua.includes('Edge')) {
    browser = 'Edge';
  }

  if (browser) {
    return `${platform} (${browser})`;
  }
  return platform;
}
