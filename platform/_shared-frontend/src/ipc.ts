// Tauri IPC bridge. Both Windows + Linux backends expose the same command
// names; the frontend calls them through this thin shim. Command vocabulary
// matches the macOS native app's controller surface so any future migration
// to a Tauri-on-mac build slots in without rewriting the frontend.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface NormalizedRegion {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface PatchPayload {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  png_data_url: string;
}

export interface MonitorInfo {
  id: string;
  name: string;
  isPrimary: boolean;
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  scaleFactor?: number;
}

export interface ScreenshotData {
  width: number;
  height: number;
  pngDataUrl: string;
}

export interface ScreenshotEntry {
  path: string;
  stem: string;
  labeled: boolean;
}

export interface LabelBox {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface LabelDocument {
  schemaVersion: 1;
  image: string;
  imageWidth: number;
  imageHeight: number;
  boxes: LabelBox[];
  /** ISO-8601 second-precision string, e.g. `2024-01-02T03:04:05Z`. */
  labeledAt: string;
}

export interface ModelUpdateReceipt {
  modelId: string;
  modelVersion: string;
  releaseSequence: number;
  artifactSha256: string;
  previousArtifactPreserved: boolean;
}

export interface CaptureTelemetry {
  capturedFrames: number;
  processedFrames: number;
  droppedFrames: number;
  copyErrors: number;
  protectedFrames: number;
  protectedContent: boolean;
  lastFrameUnixMs: number;
}

export interface DesktopCapabilityProfile {
  contractVersion: number;
  platform: string;
  supportMode: "full" | "limited";
  captureBackend: string;
  inferenceBackends: string[];
  overlayBackend: string;
  clickThroughOverlay: boolean;
  captureExclusion: boolean;
  globalHotkeys: boolean;
  localFrameProcessing: boolean;
  telemetryEnabled: boolean;
  productionTrainingRuntime: boolean;
  releaseReady: boolean;
  limitations: string[];
}

export const lb = {
  // Capture / detection lifecycle.
  getCapabilities: () => invoke<DesktopCapabilityProfile>("get_capabilities"),
  startCapture: async (monitorId: string) => {
    const actionSequence = await invoke<number>("begin_user_action");
    return invoke<void>("start_capture", { monitorId, actionSequence });
  },
  stopCapture: async () => {
    const actionSequence = await invoke<number>("begin_user_action");
    return invoke<void>("stop_capture", { actionSequence });
  },
  getCaptureTelemetry: () => invoke<CaptureTelemetry>("get_capture_telemetry"),
  setDetectionEnabled: (enabled: boolean) =>
    invoke<void>("set_detection_enabled", { enabled }),
  listMonitors: () => invoke<MonitorInfo[]>("list_monitors"),

  // Region CRUD.
  listRegions: () => invoke<NormalizedRegion[]>("list_regions"),
  addRegion: (region: NormalizedRegion) =>
    invoke<NormalizedRegion>("add_region", { region }),
  replaceRegion: (id: string, region: NormalizedRegion) =>
    invoke<void>("replace_region", { id, region }),
  deleteRegion: (id: string) => invoke<void>("delete_region", { id }),
  clearRegions: () => invoke<void>("clear_regions"),

  // Labeling pipeline.
  captureScreenshotForLabeling: () =>
    invoke<string | null>("capture_screenshot_for_labeling").catch(() => null),
  listScreenshots: () => invoke<ScreenshotEntry[]>("list_screenshots"),
  loadScreenshot: (path: string) => invoke<ScreenshotData>("load_screenshot", { path }),
  saveLabel: (path: string, doc: LabelDocument) =>
    invoke<void>("save_label", { path, doc }),
  loadLabel: (path: string) =>
    invoke<LabelDocument | null>("load_label", { path }),
  discardScreenshot: (path: string) => invoke<void>("discard_screenshot", { path }),

  // Training.
  startTraining: (epochs: number, batch: number, imgsz: number) =>
    invoke<void>("start_training", { epochs, batch, imgsz }),
  cancelTraining: () => invoke<void>("cancel_training"),

  // Authenticated ONNX updates. The backend owns the destination, keyring,
  // rollback state, production load validation, and atomic activation.
  installModelUpdate: (manifestPath: string, artifactPath: string) =>
    invoke<ModelUpdateReceipt>("install_model_update", { manifestPath, artifactPath }),

  // Window lifecycle.
  showWindow: (label: string) => invoke<void>("show_window", { label }),
  hideWindow: (label: string) => invoke<void>("hide_window", { label }),
  quit: () => invoke<void>("quit"),
};

export const events = {
  onCapabilitiesChanged: (cb: () => void) =>
    listen("capabilities-changed", () => cb()),
  onPatches: (cb: (patches: PatchPayload[]) => void) =>
    listen<PatchPayload[]>("patches-updated", (e) => cb(e.payload)),
  onRegions: (cb: (regions: NormalizedRegion[]) => void) =>
    listen<NormalizedRegion[]>("regions-updated", (e) => cb(e.payload)),
  onCaptureState: (cb: (running: boolean) => void) =>
    listen<boolean>("capture-state-changed", (e) => cb(e.payload)),
  onProtectedContent: (cb: (protectedContent: boolean) => void) =>
    listen<boolean>("protected-content-changed", (e) => cb(e.payload)),
  onCaptureError: (cb: (message: string) => void) =>
    listen<string>("capture-runtime-error", (e) => cb(e.payload)),
  onHotkeyAvailability: (cb: (available: boolean) => void) =>
    listen<boolean>("hotkey-availability-changed", (e) => cb(e.payload)),
};
