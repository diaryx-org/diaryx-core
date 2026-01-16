/**
 * CRDT module for Diaryx web app.
 *
 * This module provides integration between the Rust CRDT backend
 * and the frontend, including:
 * - Type-safe API wrapper for CRDT operations
 * - Y.Doc proxy for TipTap integration
 * - Simple sync bridge for real-time sync
 * - Workspace CRDT bridge
 */

export { RustCrdtApi, createCrdtApi } from './rustCrdtApi';
export { YDocProxy, createYDocProxy, type YDocProxyOptions } from './yDocProxy';
export { SimpleSyncBridge, createSimpleSyncBridge, type SimpleSyncBridgeOptions } from './simpleSyncBridge';
export { HocuspocusBridge, createHocuspocusBridge, type HocuspocusBridgeOptions, type ConnectionStatus } from './hocuspocusBridge';
export * from './workspaceCrdtBridge';
export * from './collaborationBridge';
