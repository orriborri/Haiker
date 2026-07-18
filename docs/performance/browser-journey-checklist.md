# Browser Journey Validation Checklist

## Purpose

This checklist validates the complete GPX-to-export user journey across all
supported browsers. Use it for:

- Pre-launch browser compatibility verification
- Post-deployment smoke testing after major UI changes
- Regression testing when updating browser support targets
- Validating Service Worker behavior and offline resilience

## When to Use

- Before each release that touches frontend code
- When adding support for a new browser version
- After updating map rendering, Service Worker, or offline logic
- When a user reports a browser-specific issue

## Supported Browser Matrix

| Browser         | Versions           | Platforms        |
|-----------------|--------------------|------------------|
| Chrome          | Latest 2 versions  | Desktop, Android |
| Firefox         | Latest 2 versions  | Desktop          |
| Safari          | Latest 2 versions  | Desktop, iOS     |
| Edge            | Latest 2 versions  | Desktop          |

---

## Test Environment Setup

1. **Local development server** running at `http://localhost:3000` (or configured URL)
2. **API server** running with test database seeded
3. **Test GPX file** prepared: use `crates/test_support/fixtures/large_route.gpx` (10,000 points)
4. **Browser DevTools** open to Network and Application tabs
5. **Disable browser extensions** that may interfere (ad blockers, privacy tools)
6. **Clear browser cache and storage** before starting each browser test
7. **Test at two viewport widths**: desktop (1440px) and mobile (375px)

---

## Journey Steps

Complete each step in order. Record Pass/Fail and any observations.

### Step A: Navigate to Application

| Item | Check |
|------|-------|
| [ ] | Application loads without console errors |
| [ ] | Layout renders correctly (no overlapping elements) |
| [ ] | Fonts and icons load properly |
| [ ] | Responsive layout at mobile breakpoint (375px) |
| [ ] | Screenshot: _reference_ |
| Notes | |

### Step B: Authenticate (Login Flow)

| Item | Check |
|------|-------|
| [ ] | Login form renders and is interactive |
| [ ] | Authentication completes successfully |
| [ ] | Redirect to authenticated state works |
| [ ] | Session persists on page reload |
| Notes | |

### Step C: Upload GPX File (10,000 points)

| Item | Check |
|------|-------|
| [ ] | File picker opens and accepts .gpx files |
| [ ] | Upload progress indicator is visible |
| [ ] | Progress updates smoothly (no freezing) |
| [ ] | Large file (10,000 points) upload completes |
| [ ] | Error state shown if upload fails (test with invalid file) |
| Notes | |

### Step D: Verify Import Completes

| Item | Check |
|------|-------|
| [ ] | Import processing indicator shown |
| [ ] | Activity appears in library when import finishes |
| [ ] | Activity metadata (name, date, distance) is correct |
| [ ] | No stale state from previous imports |
| Notes | |

### Step E: Open Activity Detail with Map

| Item | Check |
|------|-------|
| [ ] | Activity detail page loads |
| [ ] | Map renders with route overlay |
| [ ] | WebGL context acquired without errors |
| [ ] | Route geometry visible and correctly positioned |
| [ ] | Map is interactive (pan, zoom) |
| [ ] | Route stats displayed correctly |
| Notes | |

### Step F: Activity Library at Mobile Breakpoint

| Item | Check |
|------|-------|
| [ ] | Library is responsive at 375px width |
| [ ] | Activity cards stack vertically |
| [ ] | Touch targets are at least 44x44px |
| [ ] | Scrolling is smooth |
| [ ] | No horizontal overflow |
| Notes | |

### Step G: Enter Route Editor

| Item | Check |
|------|-------|
| [ ] | Editor loads with route displayed |
| [ ] | Editing tools render without overlap |
| [ ] | Tool palette is accessible (not clipped) |
| [ ] | Route points are visible and selectable |
| [ ] | Editor is responsive at both breakpoints |
| Notes | |

### Step H: Perform MovePoint Correction

| Item | Check |
|------|-------|
| [ ] | Clicking a point selects it (visual highlight) |
| [ ] | Dragging a point moves it smoothly |
| [ ] | Visual feedback is immediate (< 16 ms frame budget) |
| [ ] | Route line updates in real-time during drag |
| [ ] | Point snaps to new position on release |
| [ ] | No jank or dropped frames during interaction |
| Notes | |

### Step I: Undo / Redo

| Item | Check |
|------|-------|
| [ ] | Undo reverts the last correction |
| [ ] | Geometry visually updates on undo |
| [ ] | Redo re-applies the correction |
| [ ] | Multiple undo/redo cycles work correctly |
| [ ] | Keyboard shortcuts (Ctrl+Z / Cmd+Z) work |
| Notes | |

### Step J: Publish Corrected Version

| Item | Check |
|------|-------|
| [ ] | Publish action is available |
| [ ] | Confirmation dialog (if any) works |
| [ ] | Publishing completes with success feedback |
| [ ] | Version history shows new version |
| [ ] | Original route still accessible |
| Notes | |

### Step K: Request GPX Export

| Item | Check |
|------|-------|
| [ ] | Export button is available |
| [ ] | Progress indication shown during export generation |
| [ ] | Export does not block UI |
| [ ] | Export completes with download ready indication |
| Notes | |

### Step L: Download Exported GPX

| Item | Check |
|------|-------|
| [ ] | Download triggers browser save dialog or auto-downloads |
| [ ] | Downloaded file has .gpx extension |
| [ ] | File contains valid GPX XML |
| [ ] | Exported route matches the corrected geometry |
| [ ] | File size is reasonable for the route |
| Notes | |

### Step M: Service Worker Caching

| Item | Check |
|------|-------|
| [ ] | Service Worker registered (check DevTools > Application) |
| [ ] | Shell assets cached (HTML, CSS, JS bundles) |
| [ ] | Cache storage populated after first load |
| [ ] | Second load uses cached resources (check Network tab) |
| [ ] | Service Worker activates without errors |
| Notes | |

### Step N: Offline Simulation

| Item | Check |
|------|-------|
| [ ] | Enable offline mode in DevTools (Network > Offline) |
| [ ] | Cached pages/views load from Service Worker |
| [ ] | Read operations work from cached data |
| [ ] | Write operations queue with user feedback (pending state) |
| [ ] | Re-enable network: pending operations sync |
| [ ] | Recovery UX shows sync status |
| [ ] | No data loss during offline/online transition |
| Notes | |

### Step O: Map Tile Failure Fallback

| Item | Check |
|------|-------|
| [ ] | Block tile server requests (DevTools > Network > Block) |
| [ ] | Route geometry remains visible |
| [ ] | Neutral/fallback background shown (not blank white) |
| [ ] | Error indication for failed tiles (subtle, non-blocking) |
| [ ] | Restore tile server: tiles reload correctly |
| Notes | |

---

## Browser Results Summary

Complete one row per browser/platform combination tested.

| Browser | Version | OS | Tester | Date | Result | Notes |
|---------|---------|-----|--------|------|--------|-------|
| Chrome  |         | macOS |      |      | Pass / Fail / Partial | |
| Chrome  |         | Windows |    |      | Pass / Fail / Partial | |
| Chrome  |         | Android |    |      | Pass / Fail / Partial | |
| Firefox |         | macOS |      |      | Pass / Fail / Partial | |
| Firefox |         | Windows |    |      | Pass / Fail / Partial | |
| Safari  |         | macOS |      |      | Pass / Fail / Partial | |
| Safari  |         | iOS   |      |      | Pass / Fail / Partial | |
| Edge    |         | Windows |    |      | Pass / Fail / Partial | |

---

## Known Browser-Specific Issues

Document any known issues or workarounds per browser:

| Browser | Issue | Workaround | Status |
|---------|-------|------------|--------|
|         |       |            |        |

---

## Sign-Off

| Role           | Name | Date | Approved |
|----------------|------|------|----------|
| QA Lead        |      |      | [ ]      |
| Frontend Lead  |      |      | [ ]      |
| Product Owner  |      |      | [ ]      |
