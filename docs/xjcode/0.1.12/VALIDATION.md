# XJCode 0.1.12: personal-workspace login

Cause: IAM ListWorkspace lists team spaces. A user with a working personal space can receive Items=null, Total=0. The previous client rejected this valid empty team directory before considering the personal space.

A user-authorized isolated campus login reproduced the exact response: identity HTTP 200, team directory HTTP 200 with null Items and Total 0; personal model grants HTTP 200 with an array, including 22 authorized running text-generation entries. This is directory validation, not 22 model generation tests. No password, cookies, personal identifiers or full API response is included in this record. No access keys, conversations or workspaces were explicitly created. The browser context was closed after the read-only checks.

Fix: verify the current account before deriving the same account personal ID using the documented official portal convention; preserve team choices and existing selection; accept an explicitly empty directory; reject malformed totals/items and duplicate pages; check model grants independently; give clear empty-model guidance. No automatic personal/team workspace initialization and no account or model fallback.

Native regression: 97 passed, 0 failed. New coverage includes empty team/personal discovery, verified-account mismatch, null empty model grants, ambiguous and malformed directories, total strings and preservation/deduplication of team choices.
