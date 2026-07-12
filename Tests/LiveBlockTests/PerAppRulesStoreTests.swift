import XCTest
@testable import LiveBlock

@MainActor
final class PerAppRulesStoreTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        suiteName = "LiveBlockTests.PerAppRules.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)
        defaults.removePersistentDomain(forName: suiteName)
    }

    override func tearDown() {
        defaults.removePersistentDomain(forName: suiteName)
        defaults = nil
        suiteName = nil
        super.tearDown()
    }

    func testExclusionsPersistAndReload() {
        let key = "excluded"
        let store = PerAppRulesStore(defaults: defaults, storageKey: key)
        store.setExcluded("com.example.video", excluded: true)
        store.setExcluded("com.example.editor", excluded: true)

        let reloaded = PerAppRulesStore(defaults: defaults, storageKey: key)
        XCTAssertEqual(reloaded.excludedBundleIDs,
                       ["com.example.video", "com.example.editor"])
    }

    func testEmptyBundleIDNeverBecomesExcluded() {
        let store = PerAppRulesStore(defaults: defaults, storageKey: "excluded")
        store.setExcluded("", excluded: true)
        XCTAssertFalse(store.isExcluded(""))
        XCTAssertTrue(store.excludedBundleIDs.isEmpty)
    }

    func testToggleAndClearPersist() {
        let key = "excluded"
        let store = PerAppRulesStore(defaults: defaults, storageKey: key)
        store.toggle("com.example.game")
        XCTAssertTrue(store.isExcluded("com.example.game"))
        store.toggle("com.example.game")
        XCTAssertFalse(store.isExcluded("com.example.game"))
        XCTAssertEqual(defaults.stringArray(forKey: key), [])
    }
}
