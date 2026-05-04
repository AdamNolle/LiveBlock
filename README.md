# 🛡️ LiveBlock

**LiveBlock** is an advanced, AI-powered macOS application designed to act as a "neural ad-blocker." By intercepting display frames in real-time, detecting visual advertisements using machine learning, and seamlessly replacing them via generative inpainting, LiveBlock provides a visually pristine desktop experience across all applications.

---

## ✨ Features

- **Real-Time Display Interception**: Utilizes Apple's high-performance `ScreenCaptureKit` to capture screen frames at 60 FPS with minimal CPU overhead.
- **Neural Object Detection (WIP)**: Built to integrate with `CoreML` and quantized YOLOv11 models to identify commercial advertisements and unwanted UI elements on-screen.
- **Generative Inpainting (WIP)**: Replaces blocked content with context-aware, generated backgrounds to maintain a clean visual aesthetic.
- **Cozy Glass Overlay**: Uses a borderless, transparent `NSWindow` that floats above other apps, rendering only the modified pixels while passing mouse events through to underlying applications.
- **Interactive Control Mode**: A togglable UI state that allows users to manually drag-and-select regions on their screen for instant blocking and inpainting.

---

## 🏗️ Architecture

LiveBlock is built with a highly optimized, modular architecture in Swift 5.10+:

1. **Capture Layer** (`ScreenCaptureManager`): Manages the `SCStream`, optimizing for Apple Silicon with `32BGRA` pixel formats and a low-latency video queue.
2. **Analysis Layer** (`VisionProcessor`): The inference engine responsible for running CoreML models to detect and segment regions of interest.
3. **Modification Layer** (`InpaintingEngine`): The rendering engine that reconstructs the image buffer, removing ads and blending the inpainted results.
4. **Presentation Layer** (`OverlayWindow` & `OverlayView`): A SwiftUI-driven transparent overlay that presents the modified frames back to the user without stealing focus.

---

## 🚀 Getting Started

### Prerequisites

- **macOS 14.0+** (Apple Silicon strongly recommended for Neural Engine performance)
- **Xcode 15+**
- **XcodeGen**: Used for generating the `.xcodeproj` file. Install via Homebrew:
  ```bash
  brew install xcodegen
  ```
- **Python 3.10+** (For model exporting)

### Building the Project

1. **Clone the repository:**
   ```bash
   git clone https://github.com/AdamNolle/LiveBlock.git
   cd LiveBlock
   ```

2. **Generate the Xcode Project:**
   ```bash
   xcodegen generate
   ```
   *This reads the `project.yml` file and creates `LiveBlock.xcodeproj`.*

3. **Open and Build:**
   Open `LiveBlock.xcodeproj` in Xcode, select your Mac as the destination, and click **Build and Run**.

---

## 🧠 CoreML Model Setup

LiveBlock relies on optimized CoreML models for fast on-device inference. To generate these models from standard PyTorch weights:

1. Setup a Python virtual environment and install the required dependencies (e.g., `ultralytics`, `coremltools`).
2. Run the provided export script:
   ```bash
   python export_models.py
   ```
   *This script takes a YOLO model (e.g., `yolov8n.pt`) and exports it to a quantized `.mlpackage` utilizing W8A8 quantization and built-in Non-Maximum Suppression (NMS) for optimal Apple Neural Engine (ANE) performance.*
3. Ensure the resulting `.mlpackage` is linked within the Xcode project (typically in the `Sources` directory).

---

## 🎮 Usage

1. Launch **LiveBlock**.
2. Grant Screen Recording permissions when prompted (required by ScreenCaptureKit).
3. The app will launch in **Control Mode**, overlaying a subtle interface on your screen.
4. **Drag to select** areas you want to block, or click **Start Intercepting** to begin the neural processing loop.
5. Click **Disable Control Mode** to hide the UI and let mouse clicks pass through to your standard applications while LiveBlock runs silently over your display.

---

*LiveBlock: The future of visual sovereignty.*
