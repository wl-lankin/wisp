# Plan 4 (rename, history size, screenshot watcher): rulings and deferred items

Carried out of the subagent-driven execution ledger.

- Ruling: Task 1 may add a CoreError::Invalid(String) variant for user input errors - cheap if wrong.
- Ruling: implementers mid tier; Task 3 (watcher concurrency) on the top tier; reviewers mid tier except Task 1 and 3 on the top tier.
- Ruling: core emits Event::PeerRenamed { peer, name } on a real name change (test waits on it); Discovery::update re-registers in place without unregister (avoids goodbye packets); rename serialized with a tokio mutex; carry to Task 5: store.ts handles 'peerRenamed' (update name in place) and the TS EngineEvent type.
- Task 1: minor (deferred): Discovery::update untested (multicast test ignored); discovery lock held while sending daemon commands (fast channel sends).
- Ruling: one transaction for all stale-row deletes (items + fts), blob removal after commit with warn on non-NotFound errors; carry to Task 4: the app calls set_history_cap via spawn_blocking.
- Task 2: minor (deferred): blob removal still under the history mutex (cheap); Node-level prune path lightly tested; i64 count cast.
- Ruling: dedupe also by blake3 content hash; decode in spawn_blocking with owned bytes, PNG dimensions only; HEIC converted on macOS via `sips -s format png` to a temp file (built into macOS), skipped elsewhere; plus pending-path dedupe in the queue, xattr prefilter before the stability wait, temp-then-rename test - costs a sips call per HEIC screenshot if wrong.
- Task 3: minor (deferred): start() panics outside a tokio runtime (document); ~user tilde paths and missing $HOME fallback; Windows Modify(Any) note in tests.
- Task 3: minor (deferred): corrupt PNG body with valid header offered unchecked; failed conversion not retried in the session; macOS code compiled only by CI.
- Ruling: fix it (re-read settings after engine start), plus minors: revert the cap on save failure; skip the live cap apply when no node is running; ignore device_name changes in update_settings (rename only via rename_device); document dropped shots during restart.
- Task 4: minor (deferred): no tests for new commands/service; screenshot_folder subprocess under a std mutex on an async task; a shrink then failed save may already have pruned entries.
- Task 5: minor (deferred): Enter on an unchanged name still calls renameDevice; quick chip clicks race the save queue; "1 older items" plural; test gaps (rename error, toggle save, no-prompt shrink).
- Ruling: (1) handled at finish (PR to release so macos-latest CI runs before merge); (2) plus minors 1-5 and the T5 Enter guard and plural fix go into one fix wave.
- Final: minor (deferred): sync(true) vs concurrent sync(false) race widened by the unlocked folder lookup; denied notice once per watcher (re-shown after toggling); Discovered event carries advert addresses not the merged set; on_shot closure type inference first confirmed by macOS CI.
