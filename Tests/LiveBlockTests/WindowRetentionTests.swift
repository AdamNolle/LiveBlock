import XCTest
import AppKit
import SwiftUI
@testable import LiveBlock

/// Regression tests for the bug where the Region Editor and Render Layer
/// windows deallocated immediately after launch because they didn't set
/// `isReleasedWhenClosed = false` and were only held by `weak` refs in
/// AppController. The "New region" / "Block" / ⌘⇧B button silently no-op'd.
@MainActor
final class WindowRetentionTests: XCTestCase {

    func testRegionEditorIsNotReleasedWhenClosed() {
        let screen = NSScreen.main ?? NSScreen.screens[0]
        let panel = RegionEditorWindow(rootView: AnyView(EmptyView()),
                                       targetScreen: screen)
        XCTAssertFalse(
            panel.isReleasedWhenClosed,
            "RegionEditorWindow MUST set isReleasedWhenClosed=false or it will deallocate when closed and every entry point that opens it (⌘⇧B, the Block pill, the menu bar item, the Region Library 'New region' button) silently no-ops."
        )
    }

    func testRenderLayerIsNotReleasedWhenClosed() {
        let screen = NSScreen.main ?? NSScreen.screens[0]
        let panel = RenderLayerWindow(rootView: AnyView(EmptyView()),
                                      targetScreen: screen)
        XCTAssertFalse(
            panel.isReleasedWhenClosed,
            "RenderLayerWindow MUST set isReleasedWhenClosed=false. Otherwise the inpainted overlay disappears the first time the editor is opened+closed."
        )
    }

    func testControlPanelIsNotReleasedWhenClosed() {
        let panel = ControlPanelWindow(rootView: AnyView(EmptyView()))
        XCTAssertFalse(panel.isReleasedWhenClosed,
                       "ControlPanelWindow already had this set; this test guards against regressions.")
    }
}
