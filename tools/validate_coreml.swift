// Run the actual Vision/CoreML deployment path on a directory of holdout images.
// Usage: swift tools/validate_coreml.swift model.mlpackage images-dir output.json
import Foundation
import CoreML
import Vision
import ImageIO

guard CommandLine.arguments.count == 4 else {
    fputs("Usage: validate_coreml.swift model images-dir output.json\n", stderr)
    exit(2)
}
let source = URL(fileURLWithPath: CommandLine.arguments[1])
let directory = URL(fileURLWithPath: CommandLine.arguments[2])
let output = URL(fileURLWithPath: CommandLine.arguments[3])
do {
    let compiled = source.pathExtension == "mlmodelc" ? source : try MLModel.compileModel(at: source)
    defer { if compiled != source { try? FileManager.default.removeItem(at: compiled) } }
    let model = try VNCoreMLModel(for: MLModel(contentsOf: compiled))
    let request = VNCoreMLRequest(model: model)
    request.imageCropAndScaleOption = .scaleFit
    let files = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
        .filter { ["png", "jpg", "jpeg"].contains($0.pathExtension.lowercased()) }
        .sorted { $0.lastPathComponent < $1.lastPathComponent }
    var predictions: [String: [[String: Any]]] = [:]
    for file in files {
        try VNImageRequestHandler(url: file, orientation: .up).perform([request])
        let observations = request.results as? [VNRecognizedObjectObservation] ?? []
        predictions[file.deletingPathExtension().lastPathComponent] = observations.compactMap { observation in
            guard let label = observation.labels.first,
                  observation.confidence.isFinite, label.confidence.isFinite,
                  (0...1).contains(observation.confidence), (0...1).contains(label.confidence) else { return nil }
            let confidence = observation.confidence * label.confidence
            guard confidence >= 0.5 else { return nil }
            let rect = observation.boundingBox
            return ["class": label.identifier, "confidence": Double(confidence),
                    "x": rect.minX, "y": 1 - rect.maxY, "width": rect.width, "height": rect.height]
        }
    }
    try JSONSerialization.data(withJSONObject: predictions, options: [.sortedKeys]).write(to: output)
    print("Validated \(files.count) images through Vision/CoreML → \(output.path)")
} catch {
    fputs("CoreML validation failed: \(error.localizedDescription)\n", stderr)
    exit(1)
}
