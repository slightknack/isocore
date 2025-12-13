---
origin: generated
date: 2025-12-13
---

# The Component Model Type System

## The Abstraction Boundary

Core WebAssembly and the Component Model occupy different layers in the execution stack. Core Wasm is an instruction set—a compiler target operating on linear memory with four numeric types. The Component Model sits above this, providing a type system for inter-component communication. This isn't layering for elegance; it's a necessary separation of concerns. You cannot serialize a file handle or a mutex across a network boundary, yet you can serialize the *interface* to operations on those entities.

```wat
;; Core: instructions and numeric types
(module
  (func (export "add") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.add
  )
)

;; Component: interface types and composition
(component
  (import "logger" (func (param "msg" string) (result u32)))
)
```

The component doesn't implement `logger`—it declares that it *requires* something matching that shape. This is duck typing with a type system, contracts without inheritance.

## Resources: Handles, Not Values

A resource is a type that represents something that cannot be copied. In practice, this means operating system handles, database connections, GPU contexts—entities with identity and lifecycle semantics beyond mere data. The Component Model's resource system provides a capability-based type layer over these entities.

The syntax reflects this conceptual model. Resources live in instance types because they represent agreements between components about what exists, not standalone data definitions:

```wat
(component
  ;; An instance type declares what a provider exports
  (type $resource-provider (instance
    (export "file" (type (sub resource)))
  ))
  
  ;; Import an actual instance
  (import "filesystem" (instance $fs (type $resource-provider)))
  
  ;; Alias the resource type to use it locally
  (alias export $fs "file" (type $file))
  
  ;; Now functions can operate on this handle type
  (import "read" (func (param "f" (borrow $file)) (result (list u8))))
)
```

This ceremony isn't bureaucracy—it's encoding the topology of component boundaries. The instance import establishes *who provides the resource*. The alias brings that type into scope. The function signature declares whether ownership transfers (`own`) or remains with the caller (`borrow`).

### Ownership Semantics

```wat
(func (param "h" (own $resource)))    ;; Callee destroys the resource when done
(func (param "h" (borrow $resource))) ;; Caller retains ownership, temporary loan
```

This maps directly to the lifetime problem in systems programming. An `own` parameter means the callee is responsible for cleanup—think `close(fd)` or `drop(Arc<T>)`. A `borrow` parameter means the caller maintains responsibility—the equivalent of passing a reference in Rust or a pointer in C with the understanding that the pointer remains valid for the call's duration.

Resources cannot cross serialization boundaries because serialization implies copying, and these types represent unique identity. A file descriptor on machine A has no meaning on machine B. Your validator correctly rejects these because it's enforcing the fundamental invariant: only data crosses the wire, never capabilities.

## The Type Hierarchy

Types in the Component Model fall into three categories based on their validation rules and where they can appear.

### First-Class: Primitives and Polymorphic Containers

These types work everywhere, including direct component-level function imports:

```wat
;; Numeric and character types
bool, u8, u16, u32, u64, s8, s16, s32, s64, float32, float64, char, string

;; Polymorphic containers
(list u32)
(option string)
(result u32 (error string))
(tuple string u32 bool)
```

The polymorphic containers are particularly interesting. `option<T>` is a maybe type. `result<T, E>` is a tagged union of success and error. `list<T>` is a length-prefixed sequence. `tuple<T, U, ...>` is a fixed-arity product type. These are structural types with well-defined serialization semantics—they reduce to sequences of bytes on the wire without ambiguity.

### Second-Class: Named Structural Types

Records, variants, enums, and flags require more careful handling. In Wasmtime 39.x, these cannot appear directly in component-level function imports:

```wat
;; This fails validation in Wasmtime 39.x
(component
  (import "process" (func (param "data" (record (field "x" u32)))))
)
```

The validation failure occurs because component-level function imports have stricter type requirements in early Wasmtime versions. The workaround is to place these functions inside instance types, which use different validation rules:

```wat
;; This validates successfully
(component
  (type $api (instance
    (export "process" (func (param "data" (record (field "x" u32)))))
  ))
  (import "provider" (instance (type $api)))
)
```

Why the asymmetry? Instance types describe *interfaces*—collections of related operations and types. The component validator treats them as indivisible units, validating the instance type as a whole. Direct component imports are validated individually and subject to more restrictions. This isn't arbitrary; it reflects the reality that structured types often need accompanying context (other types, constants, or constraints) to be meaningful.

If you're on Wasmtime 39.x and need complex types, you have two strategies:

```wat
;; Strategy 1: Decompose into multiple parameters
(import "create-person" (func (param "name" string) (param "age" u32)))

;; Strategy 2: Use tuples for small fixed structures
(import "create-person" (func (param "person" (tuple string u32))))
```

The tuple approach preserves the single-argument semantics while staying within the first-class type system. The multi-parameter approach makes the structure explicit in the signature. Choose based on whether you're modeling a single concept (use tuple) or multiple independent inputs (use multiple params).

### Type References: Context Matters

The syntax for referring to types varies by context:

```wat
;; Defining a type
(type $rec (record (field "a" u32)))

;; Using it in a function parameter (direct reference)
(func (param "x" $rec))

;; Exporting a resource type (type-as-value)
(export "resource-name" (type (sub resource)))

;; Referencing an aliased type in instance exports
(export "open" (func (result (own (type $file)))))
```

The distinction between `$rec` and `(type $rec)` reflects whether you're using a type (as in a function parameter) or referring to the type itself as a first-class entity (as in resource exports). Resources use `(type ...)` because you're exporting the *type*, not a value of that type. This parallels the difference between `typeof x` and `x` in JavaScript, or `std::type_info` versus an instance in C++.

## Instance Types as Namespaces

Instance types serve a dual purpose: they group related functionality, and they establish namespace boundaries for resources and types. When you import an instance, you're importing a coherent API surface:

```wat
(component
  (type $fs-types (instance
    (export "file" (type (sub resource)))
    (export "dir" (type (sub resource)))
  ))
  
  (type $fs-ops (instance
    (export "open" (func (param "path" string) (result (own $file))))
    (export "read" (func (param "f" (borrow $file)) (result (list u8))))
  ))
  
  (import "fs:types" (instance $types (type $fs-types)))
  (import "fs:ops" (instance $ops (type $fs-ops)))
  
  (alias export $types "file" (type $file))
)
```

Notice the separation: types in one instance, operations in another. This isn't required, but it's a useful pattern when multiple instances need to share types. The filesystem types can be imported by both the filesystem operations and a filesystem watcher component, establishing a shared vocabulary.

The alias operation brings types from instance exports into component scope. Without it, `$file` is not visible—it exists only within `$types`. This is namespace hygiene: types don't leak implicitly.

## Validation Error Archeology

### "func not valid to be used as import"

This error appears when a component-level function import violates the validator's requirements. In Wasmtime 39.x, this usually means:

1. You're using a structured type (record/variant/enum/flags) directly
2. The function signature doesn't meet the component-level import constraints

The fix: wrap the function in an instance type. Instance imports are validated differently and permit structured types in their exports.

### "Resources cannot cross network boundaries"

This isn't an error in the Wasmtime validator—this is *your* validator working correctly. You've successfully compiled the WAT (syntax is valid), and now your semantic checker is doing its job: ensuring that no resource types appear in signatures that will cross serialization boundaries.

This is the validation you want. Resources represent unforgeable capabilities and local identity. Allowing them across RPC boundaries would be a type error with runtime consequences—passing a file descriptor to a remote machine, for instance.

### Parse errors: "expected `)` "

These indicate syntax mismatch. Common causes:

1. Using old resource syntax: `(resource (rep i32))` — this is deprecated
2. Wrong type reference form: `(type $x)` where `$x` is expected
3. Misplaced `(sub resource)` — valid only in type exports

When you see a parse error, you're fighting the grammar. Check the examples in this guide for the exact syntactic form.

## Empirical Methodology

The patterns in this document emerged from systematic hypothesis testing: write minimal WAT snippets, observe what compiles, observe what validates, observe what runs. This is the scientific method applied to language implementation.

When documentation is sparse or version-specific behavior is unclear, construct small test cases:

```rust
#[test]
fn hypothesis_container_types_work_inline() {
    let wat = r#"
        (component
            (import "test" (func (param "x" (list u32))))
        )
    "#;
    Component::new(&engine, wat).expect("should compile");
}
```

Start with what you know works (primitives), then incrementally add complexity (containers, then structured types). When something breaks, you've found a boundary condition. This builds a map of the implementation's actual behavior, which may diverge from specification or documentation.

Over 60 hypothesis tests were written to derive these patterns. That's not wasted effort—it's reconnaissance. You're mapping the terrain of what Wasmtime actually accepts, not what it theoretically should accept.

## Version-Specific Realities

This guide targets **Wasmtime 39.0.1**, an early Component Model implementation. Newer versions have evolved:

- Better structured type support in direct imports
- Refined resource semantics
- More complete WIT integration

If you're on a different version and patterns here don't work, the instance type workaround remains reliable. It's the lowest common denominator—wrap complex signatures in instance types and they validate across versions.

WIT (WebAssembly Interface Types) is the higher-level format. It compiles to Component Model WAT. If you're writing new code, prefer WIT:

```wit
interface filesystem {
  resource file;
  
  open: func(path: string) -> file;
  read: func(f: borrow<file>) -> list<u8>;
}
```

This generates the WAT patterns shown in this guide. But understanding the WAT is essential for debugging, because errors report against the compiled form, not the source.

## Summary Invariants

1. **Instance types are indivisible validation units** — use them for structured types and resources
2. **Resources encode capability semantics** — own vs borrow reflects responsibility transfer
3. **Type references have context-dependent syntax** — `$t` in uses, `(type $t)` in exports
4. **Container types are universally first-class** — lists, options, results, tuples work everywhere
5. **Structured types require instance boundaries** — in Wasmtime 39.x, at least
6. **Validation errors distinguish syntax from semantics** — parse errors are grammar, validation errors are constraints
7. **Version matters** — Component Model implementations evolved rapidly; what fails in 39.x may work in 50.x

When in doubt, wrap in an instance type. It's defensive programming at the type level—establishing clear boundaries for complex contracts.

---

*Built through systematic hypothesis testing against Wasmtime 39.0.1. When documentation is incomplete, empiricism provides ground truth.*
