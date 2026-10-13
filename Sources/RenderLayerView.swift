import SwiftUI

/// Renders inpainted patches, nothing else.
struct RenderLayerView: View {
    @ObservedObject var captureManager: ScreenCaptureManager
    @ObservedObject var controller: AppController

    var body: some View {
        GeometryReader { geo in
            ZStack {
                Color.clear
                if controller.captureFreeCoverEnabled {
                    ForEach(controller.regionStore.current, id: \.id) { region in
                        let rect = region.rect(in: geo.size)
                        Rectangle()
                            .fill(Color.black)
                            .frame(width: rect.width, height: rect.height)
                            .position(x: rect.midX, y: rect.midY)
                            .allowsHitTesting(false)
                    }
                } else {
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
