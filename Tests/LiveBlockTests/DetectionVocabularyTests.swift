import XCTest
@testable import LiveBlock

final class DetectionVocabularyTests: XCTestCase {

    /// A disabled class id must be dropped: `effectiveThreshold` returns nil so
    /// the detect() compactMap discards the observation.
    func testDisabledClassIsFiltered() throws {
        // Build the exact JSON shape `VocabularyHandle.get_detector_classes()`
        // emits, with class id 0 ("Logo") disabled.
        let json = """
        [
          { "enabled": false, "id": 0, "name": "Logo", "threshold": 0.25 },
          { "enabled": true, "id": 1, "name": "Ad banner", "threshold": 0.6 }
        ]
        """
        let vocab = DetectionVocabulary(detectorClassesJSON: json)

        // Disabled class -> nil (caller drops the detection regardless of score).
        XCTAssertNil(vocab.effectiveThreshold(forLabel: "Logo", globalFallback: 0.35))
        // Enabled class -> its own effective threshold.
        XCTAssertEqual(try XCTUnwrap(vocab.effectiveThreshold(forLabel: "Ad banner", globalFallback: 0.35)),
                       0.6, accuracy: 1e-6)
        // Unknown label -> global fallback (keeps a transitional model working).
        XCTAssertEqual(try XCTUnwrap(vocab.effectiveThreshold(forLabel: "person", globalFallback: 0.35)),
                       0.35, accuracy: 1e-6)
    }

    /// End-to-end through the Rust bridge: seed the vocabulary, disable a class
    /// via the FFI, then confirm the Swift lookup drops it.
    func testDisabledClassThroughBridge() throws {
        let handle = VocabularyHandle()
        let vocabJSON = """
        {"version":1,"classes":[
          {"id":0,"name":"Logo","prompts":["logo"]},
          {"id":1,"name":"Ad banner","prompts":["advertisement"]}
        ]}
        """
        XCTAssertTrue(handle.set_vocabulary(vocabJSON))
        XCTAssertTrue(handle.set_class_enabled(0, false))

        let classesJSON = handle.get_detector_classes().toString()
        XCTAssertTrue(classesJSON.contains("\"name\": \"Logo\""))

        let vocab = DetectionVocabulary(detectorClassesJSON: classesJSON)
        // "Logo" was disabled across the FFI boundary -> filtered.
        XCTAssertNil(vocab.effectiveThreshold(forLabel: "Logo", globalFallback: 0.35))
        // "Ad banner" stays enabled.
        XCTAssertNotNil(vocab.effectiveThreshold(forLabel: "Ad banner", globalFallback: 0.35))
    }
}
