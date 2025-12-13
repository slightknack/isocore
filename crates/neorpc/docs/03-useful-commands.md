---
origin: handwritten
date: 2025-12-12
---

Copy all source code to clipboard:

```
find src -name "*.rs" -print0 | xargs -0 -I {} sh -c 'echo "// File: $(basename {})" && cat "{}"' | pbcopy
```

Copy cargo test results to clipboard:

```
cargo test 2>&1 | pbcopy
```
