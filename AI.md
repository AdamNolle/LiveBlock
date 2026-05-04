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
- Responsible for detecting "blocks" (ads, UI elements) within the frame.
- Integrates with CoreML models (YOLO).
- **Current State**: Mock implementation for YOLOv11-OBB detection.
- **Goal**: Use SAM 2 (Segment Anything Model) for pixel-perfect mask generation.

### 3. Modification Layer (`InpaintingEngine.swift`)
- Takes the original frame and the detected bounding boxes.
- Performs "inpainting" to fill in the blocked areas.
- **Current State**: Mock implementation using color fills.
- **Goal**: Use Generative AI (Stable Diffusion or custom GANs) for seamless reconstruction.

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

- [LiveBlockApp.swift](file:///Users/adamnolle/Desktop/LiveBlock/Sources/LiveBlockApp.swift): Entry point and app lifecycle.
- [ScreenCaptureManager.swift](file:///Users/adamnolle/Desktop/LiveBlock/Sources/ScreenCaptureManager.swift): Core logic for frame interception.
- [VisionProcessor.swift](file:///Users/adamnolle/Desktop/LiveBlock/Sources/VisionProcessor.swift): Detection and segmentation logic.
- [InpaintingEngine.swift](file:///Users/adamnolle/Desktop/LiveBlock/Sources/InpaintingEngine.swift): Frame modification logic.
- [project.yml](file:///Users/adamnolle/Desktop/LiveBlock/project.yml): XcodeGen project specification.

## Development Workflow

1. **Project Generation**: The project uses `xcodegen`. If you modify `project.yml`, run `xcodegen generate` to update the `.xcodeproj`.
2. **Model Export**: Use `export_models.py` to convert YOLO `.pt` models to CoreML `.mlpackage` format.

## AI Agent Tips

- When debugging frame drops, check the `videoQueue` QoS and the `ciContext` reuse in `ScreenCaptureManager`.
- The `OverlayWindow` is designed to be "non-activating" to avoid stealing focus from other apps.
- If adding new assets or sources, always update `project.yml` first.

---
*LiveBlock: The future of visual sovereignty.*
