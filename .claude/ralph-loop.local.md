---
active: true
iteration: 17
session_id: 12f423aa-fbba-43f3-9f4b-d27cd5a78070
max_iterations: 80
completion_promise: "ALL STORIES PASS"
started_at: "2026-09-11T19:30:54Z"
---

Work through ralph/prd.json for cctop. Each iteration: (1) read ralph/prd.json and ralph/progress.txt; (2) take the first story with passes=false, in priority order; (3) implement it fully in the working tree, following the PRD in tasks/prd-cctop.md for detail and matching the surrounding code; (4) run cargo fmt, cargo check, cargo clippy --all-targets -- -D warnings and cargo test and fix until green — never mark a story done with failing checks; (5) set passes=true in ralph/prd.json, append '[US-XXX] PASS | summary' to ralph/progress.txt (or FAIL with the blocker), and git commit on branch ralph/cctop-v1 with message 'US-XXX: <title>'; (6) do only one story per iteration. When every story has passes=true, output <promise>ALL STORIES PASS</promise>.
