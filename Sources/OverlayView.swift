import SwiftUI
import CoreMedia

struct OverlayView: View {
    @StateObject private var captureManager = ScreenCaptureManager()
    @State private var isControlModeActive: Bool = true
    
    // Drag-to-select state
    @State private var dragStart: CGPoint? = nil
    @State private var currentDragRect: CGRect? = nil
    @State private var userRegions: [CGRect] = []
    
    var body: some View {
        ZStack {
            // 1. The Captured & Inpainted Frame (Rendered securely)
            // We only render the full frame if we are actively capturing and have content.
            // However, to act as a proper glass overlay, we only want to show the inpainted regions.
            // For this mock, we will display the inpainted frame, but the logic will be optimized in the manager.
            if let frame = captureManager.currentInpaintedFrame {
                Image(nsImage: frame)
                    .resizable()
                    // Use scale-to-fill so the cropped screen matches the window
                    .aspectRatio(contentMode: .fill)
            } else {
                Color.clear
            }
            
            // Draw user selected regions
            ForEach(0..<userRegions.count, id: \.self) { index in
                Rectangle()
                    .stroke(Color.red, style: StrokeStyle(lineWidth: 2, dash: [5]))
                    .background(Color.red.opacity(0.2))
                    .frame(width: userRegions[index].width, height: userRegions[index].height)
                    .position(x: userRegions[index].midX, y: userRegions[index].midY)
            }
            
            // Draw current drag rect
            if let rect = currentDragRect {
                Rectangle()
                    .stroke(Color.blue, lineWidth: 2)
                    .background(Color.blue.opacity(0.3))
                    .frame(width: rect.width, height: rect.height)
                    .position(x: rect.midX, y: rect.midY)
            }
            
            // 2. Controls and UI Overlay
            if isControlModeActive {
                VStack {
                    HStack {
                        Text("LiveBlock Ad-Blocker")
                            .font(.headline)
                            .foregroundColor(.white)
                            .padding(8)
                            .background(Color.black.opacity(0.6))
                            .cornerRadius(8)
                        
                        Spacer()
                        
                        Button(action: {
                            toggleControlMode()
                        }) {
                            Text("Disable Control Mode")
                                .bold()
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(.red)
                    }
                    .padding()
                    
                    Spacer()
                    
                    if !captureManager.isRunning {
                        Button("Start Intercepting") {
                            Task {
                                await captureManager.startCapture()
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.large)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.black.opacity(isControlModeActive ? 0.1 : 0.001)) // Very faint background so it receives clicks
        .border(Color.white.opacity(isControlModeActive ? 0.5 : 0.1), width: 1) // Small border as requested
        .gesture(
            DragGesture(minimumDistance: 5)
                .onChanged { value in
                    if dragStart == nil {
                        dragStart = value.startLocation
                    }
                    if let start = dragStart {
                        let rect = CGRect(
                            x: min(start.x, value.location.x),
                            y: min(start.y, value.location.y),
                            width: abs(value.location.x - start.x),
                            height: abs(value.location.y - start.y)
                        )
                        currentDragRect = rect
                    }
                }
                .onEnded { value in
                    if let rect = currentDragRect, rect.width > 20, rect.height > 20 {
                        userRegions.append(rect)
                        captureManager.updateUserRegions(userRegions)
                    }
                    dragStart = nil
                    currentDragRect = nil
                }
        )
        // A global shortcut or menubar item should re-enable Control Mode.
        // For demonstration, we'll listen for a global hotkey or just rely on a timer/menu bar in the future.
    }
    
    private func toggleControlMode() {
        isControlModeActive.toggle()
        if let window = NSApplication.shared.windows.first(where: { $0 is OverlayWindow }) as? OverlayWindow {
            window.isControlModeActive = isControlModeActive
        }
    }
}
