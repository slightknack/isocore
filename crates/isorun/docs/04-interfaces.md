---
origin: handwritten
date: 2025-12-12
---

when we register a WIT interface we need to:

- traverse it and extract all:
  - functions, 
  - their names, 
  - their argument types, 
  - and their return types

when we call a remote function we need to:

- encode all types to neopack with val
- queue it for transmission, yield

when we receive a remote result we need to:

- dequeue the result, and yield
- continue or trap depending on the result of resume.
