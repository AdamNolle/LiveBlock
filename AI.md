# LiveBlock: AI-First Documentation

Welcome, fellow AI agent. This document is designed to give you a deep understanding of the **LiveBlock** project, its architecture, and its technical goals.

## Project Overview

**LiveBlock** is a macOS application designed to intercept, analyze, and modify display frames in real-time. Its primary use case is to act as a "neural ad-blocker" that visually removes advertisements from the screen and replaces them with inpainted backgrounds, creating a cleaner viewing experience across all applications.

## Technical Stack

- **Language**: Swift 5.10+
- **Frameworks**: SwiftUI, ScreenCaptureKit, CoreML, Vision, AppKit
- **Capture Engine**: `ScreenCaptureKit` (Apple's high-performance screen recording framework)
- **Vision Engine**: `CoreML` (running YOLOv8/YOLOv11 for detection)
- **Rendering**: `CIContext` and `Metal` for real-time frame manipulation

## Architecture

The project follows a modular architecture centered around a high-performance video processing pipeline.

### 1. Capture Layer (`ScreenCaptureManager.swift`)
- Uses `SCStream` to capture the display.
- Operates at 60 FPS with a low-latency queue (`videoQueue`).
- Optimized for Apple Silicon using `32BGRA` pixel formats.
- Streams `CVPixelBuffer` frames to the processing layer.

### 2. Analysis Layer (`VisionProcessor.swift`)
- Loads the bundled `yolov8n.mlpackage` via `VNCoreMLModel` and runs `VNCoreMLRequest` on the capture queue.
- **Current State**: The shipped weights are **generic YOLOv8n** trained on COCO (80 classes — `person`, `car`, `dog`, …). They are **not** trained to detect advertisements. Detection mode demonstrates the pipeline end-to-end; it does not actually identify ads. See `ASSESSMENT.md` §3 for the path to a real ad/logo detector (OpenLogo / LogoDet-3K fine-tune, or GroundingDINO + SigLIP zero-shot).
- **Goal**: Replace the COCO weights with an ad/logo-specific detector or a SAM 2-based segmentation model.

### 3. Modification Layer (`InpaintingEngine.swift`)
- Replaces target regions using **edge-color extrapolation** (sample the border, blur, composite). A `CVPixelBufferPool` keeps allocations off the hot path.
- **Current State**: Lightweight, no ML weights. Visually acceptable for solid backgrounds; obvious on textured backgrounds.
- **Goal**: Port Telea or LaMa/MAT to Metal Performance Shaders for true content-aware inpainting (`ASSESSMENT.md` §4, §8 Phase 7).

### 4. Presentation Layer (`OverlayWindow.swift` & `OverlayView.swift`)
- Creates a transparent, full-screen (or windowed) overlay.
- Uses `NSHostingView` to render SwiftUI content.
- Displays the processed/inpainted frames back to the user.
- **Control Mode**: A state where the window accepts mouse events (allowing drag-to-select) or passes them through to the underlying apps.
- **Interactive Selection**: Users can drag on the overlay to manually define regions for blocking/inpainting.

## Key Features

- **High-Performance Interception**: Using ScreenCaptureKit ensures minimal CPU overhead compared to legacy CGWindowList methods.
- **Visual Privacy**: The overlay window sets `sharingType = .none` so it doesn't capture itself in a loop.
- **Dynamic Interaction**: The `OverlayWindow` can toggle `ignoresMouseEvents` to switch between an interactive control surface and a transparent "glass" layer.

## Key Files

- `Sources/LiveBlockApp.swift`: Entry point and app lifecycle.
- `Sources/ScreenCaptureManager.swift`: Core logic for frame interception.
- `Sources/VisionProcessor.swift`: Detection logic (CoreML / Vision).
- `Sources/InpaintingEngine.swift`: Frame modification logic.
- `Sources/RegionStore.swift`: Persistent normalized user-drawn regions.
- `Sources/MenuBarController.swift`: Status-bar menu (start/stop, control mode, quit).
- `Sources/HotKeyMonitor.swift`: Global hotkeys (⌘⇧B / ⌘⇧L).
- `project.yml`: XcodeGen project specification.
- `ASSESSMENT.md`: Standing audit + cross-platform roadmap.

## Development Workflow

1. **Project Generation**: The project uses `xcodegen`. If you modify `project.yml`, run `xcodegen generate` to update the `.xcodeproj`.
2. **Model Export**: Use `tools/export_models.py` to convert YOLO `.pt` weights (in `models/`) to CoreML `.mlpackage` format.

## AI Agent Tips

- When debugging frame drops, check the `videoQueue` QoS and the `ciContext` reuse in `ScreenCaptureManager`.
- The `OverlayWindow` is designed to be "non-activating" to avoid stealing focus from other apps.
- If adding new assets or sources, always update `project.yml` first.

---
*LiveBlock: The future of visual sovereignty.*
