# Complete Apple notarization

The universal Mac app is signed with Developer ID Application for team
`RSDJXF7DXN`. Apple notarization still needs account credentials saved locally.

1. Sign in to [your Apple Account](https://account.apple.com/) yourself. In
   Sign-In and Security, choose App-Specific Passwords and generate one for
   LiveBlock notarization. Apple requires two-factor authentication for this;
   see [Apple's instructions](https://support.apple.com/en-us/102654).
2. In Terminal, run:

   ```sh
   xcrun notarytool store-credentials LiveBlock --team-id RSDJXF7DXN
   ```

   Enter the Apple Account associated with this developer team and the generated
   app-specific password at the local prompts. Keep the password out of chat,
   source files and command history. The tool validates and saves the profile
   in your Keychain.
3. Tell Codex **saved** once that command succeeds. The remaining submission is
   already authorized by your request to notarize the app. To run it yourself:

   ```sh
   cd /Users/adamnolle/Desktop/Code/LiveBlock
   tools/release_macos.sh --notarize LiveBlock
   ```

The release script requires Apple's `Accepted` result, staples the ticket, and
checks it with `stapler` and Gatekeeper before producing
`~/Library/Caches/LiveBlock/release/LiveBlock-notarized.zip`.
The existing `LiveBlock-signed.zip` has not been notarized.

Reference: [Apple's notarization documentation](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).
