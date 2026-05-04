import Foundation
import ScreenCaptureKit
import CoreGraphics
import AppKit

@MainActor
class ScreenCaptureManager: NSObject, ObservableObject, SCStreamOutput, SCStreamDelegate {
    
    @Published var currentInpaintedFrame: NSImage? = nil
    @Published var isRunning: Bool = false
    
    private var stream: SCStream?
    nonisolated private let visionProcessor = VisionProcessor()
    nonisolated private let inpaintingEngine = InpaintingEngine()
    
    // Video processing queue
    nonisolated private let videoQueue = DispatchQueue(label: "com.liveblock.videoQueue", qos: .userInteractive)
    
    // Performance: Re-use CIContext to prevent beachballing
    nonisolated private let ciContext = CIContext()
    
    // User selected regions for mock targeting
    final class RegionsStorage: @unchecked Sendable {
        private let lock = NSLock()
        private var regions: [CGRect] = []
        
        var current: [CGRect] {
            lock.lock()
            defer { lock.unlock() }
            return regions
        }
        
        func update(_ newRegions: [CGRect]) {
            lock.lock()
            regions = newRegions
            lock.unlock()
        }
    }
    
    nonisolated private let regionsStorage = RegionsStorage()
    
    func updateUserRegions(_ regions: [CGRect]) {
        regionsStorage.update(regions)
    }
    
    func startCapture() async {
        do {
            // Request permissions implicitly by accessing shareable content
            let availableContent = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
            
            // For this app, we will capture the main display. 
            // In a full implementation, you'd find the display that intersects with the OverlayWindow.
            guard let display = availableContent.displays.first else {
                print("No displays found.")
                return
            }
            
            let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
            
            let configuration = SCStreamConfiguration()
            configuration.width = display.width
            configuration.height = display.height
            configuration.showsCursor = true
            
            // 60 FPS performance budget
            configuration.minimumFrameInterval = CMTime(value: 1, timescale: 60)
            configuration.queueDepth = 6 // Minimize stale frames
            
            // Apple Silicon Optimization: Use CVPixelBuffer formats ideal for ANE/Metal
            configuration.pixelFormat = kCVPixelFormatType_32BGRA
            
            stream = SCStream(filter: filter, configuration: configuration, delegate: self)
            try stream?.addStreamOutput(self, type: .screen, sampleHandlerQueue: videoQueue)
            
            try await stream?.startCapture()
            
            self.isRunning = true
            print("Screen Capture Started Successfully.")
            
        } catch {
            print("Failed to start capture: \(error.localizedDescription)")
        }
    }
    
    func stopCapture() async {
        do {
            try await stream?.stopCapture()
            self.isRunning = false
        } catch {
            print("Failed to stop capture: \(error.localizedDescription)")
        }
    }
    
    // MARK: - SCStreamOutput
    nonisolated func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen,
              let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        
        // 1. Detection (Use user selected regions mapped to pixel buffer dimensions)
        let width = CGFloat(CVPixelBufferGetWidth(pixelBuffer))
        let height = CGFloat(CVPixelBufferGetHeight(pixelBuffer))
        
        // Map user regions from an assumed 800x600 view coordinate space to the full buffer
        // In a production app, we would use the exact sourceRect of the window.
        let detectedAds = regionsStorage.current.map { rect in
            let scaleX = width / 800.0 // Default window width
            let scaleY = height / 600.0 // Default window height
            let scaledRect = CGRect(x: rect.minX * scaleX, y: rect.minY * scaleY, width: rect.width * scaleX, height: rect.height * scaleY)
            return AdBoundingBox(rect: scaledRect, confidence: 1.0, brandClass: "User Selected")
        }
        
        guard !detectedAds.isEmpty else {
            // Render clear frame if no ads, so the user can still see through
            let ciImage = CIImage(cvPixelBuffer: pixelBuffer)
            if let cgImage = self.ciContext.createCGImage(ciImage, from: ciImage.extent) {
                let nsImage = NSImage(cgImage: cgImage, size: NSSize(width: cgImage.width, height: cgImage.height))
                Task { @MainActor in
                    self.currentInpaintedFrame = nsImage
                }
            }
            return
        }
        
        // 2. Generative Reconstruction (Mocked RETHINED)
        let inpaintedBuffer = inpaintingEngine.inpaint(frame: pixelBuffer, ads: detectedAds)
        
        // Convert to NSImage for SwiftUI
        let ciImage = CIImage(cvPixelBuffer: inpaintedBuffer)
        if let cgImage = self.ciContext.createCGImage(ciImage, from: ciImage.extent) {
            let nsImage = NSImage(cgImage: cgImage, size: NSSize(width: cgImage.width, height: cgImage.height))
            
            Task { @MainActor in
                self.currentInpaintedFrame = nsImage
            }
        }
    }
}
