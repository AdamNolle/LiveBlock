import SwiftUI

/// Renders inpainted patches, nothing else.
struct RenderLayerView: View {
    @ObservedObject var captureManager: ScreenCaptureManager

    var body: some View {
        GeometryReader { geo in
            ZStack {
                Color.clear
                ForEach(captureManager.currentPatches.indices, id: \.self) { idx in
                    let patch = captureManager.currentPatches[idx]
                    let rect = denormalize(patch.normalizedRect, in: geo.size)
                    Image(decorative: patch.image, scale: 1.0, orientation: .up)
                        .resizable()
                        .interpolation(.low)
                        .frame(width: rect.width, height: rect.height)
                        .position(x: rect.midX, y: rect.midY)
                        .allowsHitTesting(false)
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .allowsHitTesting(false)
        }
        .ignoresSafeArea()
    }

    private func denormalize(_ rect: CGRect, in size: CGSize) -> CGRect {
        CGRect(x: rect.minX * size.width,
               y: rect.minY * size.height,
               width: rect.width * size.width,
               height: rect.height * size.height)
    }
}
