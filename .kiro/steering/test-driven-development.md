# Test-Driven Development (TDD)

This project follows strict Test-Driven Development practices. All production code must be written in response to a failing test.

## The Red-Green-Refactor Cycle

TDD is built on a repeating three-step cycle. **Never skip any step.**

### Step 1: RED — Write a Failing Test

Before writing any production code, write a test that:

1. **Describes the behavior you want to implement** — Focus on *what* the code should do, not *how*.
2. **Fails for the right reason** — The test should fail because the behavior doesn't exist yet, not because of a syntax error or compilation issue.
3. **Is minimal** — Write the simplest test that will fail. Don't test edge cases yet.

```bash
# Run the test to confirm it fails
cargo test test_name -- --nocapture
```

**Red Step Checklist:**
- [ ] Test file exists in the appropriate location
- [ ] Test clearly describes expected behavior in its name
- [ ] Test compiles successfully
- [ ] Test fails with a clear assertion error
- [ ] Commit message includes "RED:" prefix (optional, for WIP commits)

### Step 2: GREEN — Write the Minimal Code to Pass

Write the **minimum amount of production code** needed to make the test pass.

1. **Resist the urge to write "proper" code** — Hardcoded values, simple implementations, and shortcuts are encouraged.
2. **Focus only on making the test pass** — Don't anticipate future requirements.
3. **Don't add functionality not covered by tests** — YAGNI (You Aren't Gonna Need It).

```bash
# Run the test to confirm it passes
cargo test test_name -- --nocapture
```

**Green Step Checklist:**
- [ ] Production code compiles without warnings
- [ ] Test passes
- [ ] No other tests are broken (`cargo test`)
- [ ] Clippy passes (`cargo clippy -- -D warnings`)
- [ ] Code is formatted (`cargo fmt`)

### Step 3: REFACTOR — Improve the Code

**This step is mandatory, not optional.** With tests passing, now improve the code while keeping behavior unchanged.

1. **Remove duplication** — Look for repeated logic, magic numbers, and redundant code.
2. **Improve naming** — Rename variables, functions, and types to clearly express intent.
3. **Simplify** — Apply design patterns only if they genuinely improve clarity.
4. **Handle edge cases** — Add tests for edge cases first (go back to RED), then handle them.

```bash
# Continuously run tests during refactoring
cargo watch -x test

# Or manually after each change
cargo test
```

**Refactor Step Checklist:**
- [ ] All duplication removed (DRY principle)
- [ ] Names are clear and intention-revealing
- [ ] Code is easy to read and understand
- [ ] Functions are small and focused
- [ ] No commented-out code
- [ ] All tests still pass
- [ ] Clippy still passes
- [ ] Code is formatted

## Refactoring Techniques

Common refactoring patterns to apply:

### Extract Function
When a function is doing multiple things, extract parts into well-named helper functions.

```rust
// Before: One long function
fn process_route(route: Route) -> ProcessedRoute {
    let validated = validate(route)?;
    let enriched = enrich_with_geodata(validated)?;
    let scored = calculate_difficulty_score(enriched);
    Ok(scored)
}

// After: Clear, composable functions
fn process_route(route: Route) -> ProcessedRoute {
    validate(route)
        .and_then(enrich_with_geodata)
        .map(calculate_difficulty_score)
}
```

### Extract Struct/Type
When a group of values are always used together, create a type.

```rust
// Before: Related values scattered
fn calculate_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64

// After: Domain type
struct Coordinates { latitude: f64, longitude: f64 }
fn calculate_distance(from: Coordinates, to: Coordinates) -> f64
```

### Replace Magic Values with Named Constants
```rust
// Before
if distance > 42.195 { ... }

// After
const MARATHON_DISTANCE_KM: f64 = 42.195;
if distance > MARATHON_DISTANCE_KM { ... }
```

### Remove Dead Code
Delete unused functions, commented code, and unnecessary abstractions. Tests protect you.

## Test Organization

### Unit Tests

Place unit tests in the same file as the code being tested, within a `#[cfg(test)]` module:

```rust
// src/route_editing/mod.rs
pub fn calculate_distance(a: &Point, b: &Point) -> f64 {
    // implementation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculate_distance_returns_zero_for_same_point() {
        let point = Point { lat: 45.0, lon: 9.0 };
        assert_eq!(calculate_distance(&point, &point), 0.0);
    }
}
```

### Integration Tests

Place integration tests in the `tests/` directory:

```rust
// tests/api/routes_test.rs
use haiker_test_support::TestApp;

#[tokio::test]
async fn create_route_returns_201() {
    let app = TestApp::spawn().await;
    let response = app.post_route(valid_route_payload()).await;
    assert_eq!(response.status(), 201);
}
```

### Domain Tests

Domain logic tests go in `crates/app/src/{context}/tests.rs` or inline in the module.

## TDD Best Practices

### 1. Write the Assert First
Start with the assertion, then work backwards to arrange the test.

```rust
#[test]
fn route_total_distance_sums_all_segments() {
    // Arrange (add this next)
    let route = Route::new(vec![
        segment_of_length(5.0),
        segment_of_length(3.0),
    ]);
    
    // Assert (write this first)
    assert_eq!(route.total_distance(), 8.0);
}
```

### 2. One Concept Per Test
Each test should verify one specific behavior.

```rust
// Good: One concept per test
#[test]
fn empty_route_has_zero_distance() { ... }

#[test]
fn single_segment_route_returns_segment_length() { ... }

// Bad: Multiple concepts in one test
#[test]
fn route_distance_works() {
    // tests empty, single, and multiple segments
}
```

### 3. Use Test Builders
Create readable test data with builder patterns:

```rust
fn a_route() -> RouteBuilder {
    RouteBuilder::default()
}

fn segment_of_length(km: f64) -> Segment {
    SegmentBuilder::new().with_length(km).build()
}

#[test]
fn complex_route_scenario() {
    let route = a_route()
        .with_name("Morning Run")
        .with_segment(segment_of_length(5.0))
        .with_segment(segment_of_length(3.0))
        .build();
}
```

### 4. Test Behavior, Not Implementation
Tests should survive refactoring. Focus on inputs and outputs, not internal state.

```rust
// Good: Tests behavior
#[test]
fn cancel_route_removes_it_from_active_routes() {
    let routes = Routes::new();
    let id = routes.create("Test Route");
    routes.cancel(id);
    assert!(!routes.is_active(id));
}

// Bad: Tests implementation details
#[test]
fn cancel_route_sets_cancelled_flag_to_true() {
    let route = Route::new();
    route.cancel();
    assert!(route.cancelled_flag); // Internal detail
}
```

### 5. Descriptive Test Names
Use test names that describe the behavior being tested:

```rust
// Good
#[test]
fn reject_import_of_duplicate_route() { }
#[test]
fn calculate_elevation_gain_ignores_descending_segments() { }

// Bad
#[test]
fn test_import() { }
#[test]
fn test_elevation() { }
```

## Common TDD Patterns

### Triangulation
Start with one test case, then add more to drive generalization:

```rust
#[test]
fn sum_of_two_numbers() {
    assert_eq!(add(2, 3), 5);
}

// Green: hardcode return 5

#[test]
fn sum_of_different_numbers() {
    assert_eq!(add(4, 7), 11);
}

// Now must implement actual logic
```

### Fake It 'Til You Make It
In the GREEN step, return the exact value the test expects:

```rust
#[test]
fn first_route_number_is_one() {
    let counter = RouteCounter::new();
    assert_eq!(counter.next(), 1);
}

// GREEN implementation
impl RouteCounter {
    pub fn next(&self) -> u32 {
        1
    }
}

// REFACTOR: Now generalize after second test
```

### Boundary Tests
After the happy path, test boundaries:

```rust
#[test]
fn empty_route_list_is_valid() { }

#[test]
fn route_name_cannot_exceed_255_characters() { }

#[test]
fn route_with_zero_segments_is_valid() { }
```

## TDD Workflow Summary

```
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│   ┌─────────┐      ┌─────────┐      ┌────────────┐        │
│   │  RED    │─────▶│  GREEN  │─────▶│  REFACTOR  │        │
│   │         │      │         │      │            │        │
│   │ Write   │      │ Write   │      │ Improve    │        │
│   │ Failing │      │ Minimal │      │ Design     │        │
│   │ Test    │      │ Code    │      │ Without    │        │
│   │         │      │         │      │ Changing   │        │
│   │         │      │         │      │ Behavior   │        │
│   └─────────┘      └─────────┘      └────────────┘        │
│        ▲                                   │               │
│        └───────────────────────────────────┘               │
│                    (Next Test)                             │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## When TDD Feels Hard

If TDD feels difficult, it often indicates:

1. **The test is too big** — Break it into smaller tests
2. **The design is wrong** — Listen to the pain, refactor the design
3. **Missing abstractions** — Extract types and functions
4. **Testing implementation** — Refocus on behavior, not internals
5. **Skipping refactor step** — Debt accumulates, making tests harder

## Mandatory Checklist Before Every Commit

- [ ] All tests pass (`cargo test`)
- [ ] Clippy passes (`cargo clippy -- -D warnings`)
- [ ] Code formatted (`cargo fmt`)
- [ ] Refactoring step completed
- [ ] No commented-out code
- [ ] Test names clearly describe behavior
- [ ] Production code has no logic untested

## References

- *Test Driven Development: By Example* — Kent Beck
- *Clean Code* — Robert C. Martin
- *Refactoring* — Martin Fowler
