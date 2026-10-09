# Plan 3 (brand, installers and CI): rulings and deferred items

Carried out of the subagent-driven execution ledger.

- Ruling: hand-made SVG logo instead of Gemini generation (Gemini MCP not connected) - vector art stays crisp at tray sizes - the user may still want a generated alternative.
- Ruling: implementers on the mid tier, reviewers mid tier except Task 3 and 5 (installer/CI security) on the top tier.
- Ruling: execution starts right after writing the plan because the user said "continue with the plan" - the user can still change the plan mid-way.
- Task 1: minor (deferred): seenErrorId starts at 0 (one spurious wake possible if notices exist at mount); small duplication; clipboard match is a substring.
- Task 2: minor (deferred): whitespace-only lines in tray SVGs; loose preview heading; not checked in a real tray/menu bar.
- Task 3: minor (deferred): rule name "Wisp" is generic (delete may remove user/Windows-created rules; use a distinctive name); netsh results not popped/checked; optional remoteip=localsubnet; install/uninstall and macOS dmg not run on real systems.
- Task 4: minor (deferred): Cargo.lock not updated by set (README must say: run cargo check after set-version and commit Cargo.lock; carried to Task 6); check does not cover lockfiles; lockfile edit silent when regex misses; readVersions throw gives a stack trace; JSON regex expects two-space indent.
- Ruling: pin all actions in both workflows to the SHAs the reviewer resolved (with version comments; rust-toolchain gets toolchain: stable), add .github/dependabot.yml for github-actions weekly, release.yml rust-cache save-if false, bundle.macOS.signingIdentity "-" (ad-hoc) so unsigned Apple silicon builds are not reported as damaged, and release body text with the macOS 15 "Privacy & Security > Open Anyway" instruction - costs Dependabot PR noise if wrong.
- Task 5: minor (deferred): possible double draft if both legs finish together (optional create-release job); CI runs twice for push+PR on listed branches; checkout/setup-node stay on v4 (Node 20 deprecation warnings).
- Task 6: minor (deferred): cd ambiguity in the test commands block; WebView2 note unverified; save folder not mentioned; loose catch card wording.
- Ruling: one fix wave for Important 1-3 plus cheap minors (README "not code signed by a trusted certificate", "push the commit and the tag", spec section 7 macOS line updated to Open Anyway, @types/node ^22); keep the generic "Wisp" firewall rule name and do not add remoteip=localsubnet (would break add-by-IP across segments).
- Final: minor (deferred): later-created windows re-show an old uncleared clipboard error from their first snapshot; rare duplicate notice race (reduce should skip e.seq <= clipboardErrorShown); release workflow runs no tests (tag only CI-green commits); no release concurrency group; rust-toolchain branch pin needs manual refresh; MSVC/Xcode prerequisites not listed.
