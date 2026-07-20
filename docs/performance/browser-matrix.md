# Browser Compatibility Matrix

This document defines which browsers are officially supported by Haiker, including minimum version thresholds and the rationale behind each requirement.

## Supported Browsers

### Desktop

| Browser         | Supported Versions | Minimum Version |
|-----------------|--------------------|-----------------|
| Chrome          | Latest 2 versions  | 95+             |
| Firefox         | Latest 2 versions  | 100+            |
| Safari (macOS)  | Latest 2 versions  | 15.4+           |
| Edge            | Latest 2 versions  | 95+             |

### Mobile

| Browser              | Supported Versions | Minimum Version |
|----------------------|--------------------|-----------------|
| Chrome (Android)     | Latest 2 versions  | 95+             |
| Safari (iOS)         | Latest 2 versions  | 15.4+           |

## Required Web APIs

The minimum version thresholds above are determined by the following Web API requirements:

### WebGL2 (required by MapLibre GL JS 4.x)

MapLibre GL JS 4.x requires WebGL2 for map rendering. This is the primary constraint that sets our minimum browser versions.

- Chrome: WebGL2 supported since version 56
- Firefox: WebGL2 supported since version 51
- Safari: WebGL2 supported since version 15
- Edge: WebGL2 supported since version 79

### Service Workers (app shell caching)

Haiker uses Service Workers to cache the application shell for offline-capable loading and improved repeat-visit performance.

- Chrome: supported since version 40
- Firefox: supported since version 44
- Safari: supported since version 11.1
- Edge: supported since version 17

### IndexedDB (local draft recovery)

IndexedDB is used for local draft recovery, ensuring unsaved route edits are not lost on unexpected page closure or network interruption.

- Chrome: supported since version 24
- Firefox: supported since version 16
- Safari: supported since version 10
- Edge: supported since version 12

### CSS Grid and Flexbox

The layout system relies on CSS Grid and Flexbox for responsive design across the application.

- Chrome: CSS Grid since version 57, Flexbox since version 29
- Firefox: CSS Grid since version 52, Flexbox since version 28
- Safari: CSS Grid since version 10.1, Flexbox since version 9
- Edge: CSS Grid since version 16, Flexbox since version 12

### ES2020+ (async/await, optional chaining, nullish coalescing)

The frontend codebase uses modern JavaScript features including async/await, optional chaining (`?.`), and nullish coalescing (`??`).

- Chrome: full ES2020 support since version 80
- Firefox: full ES2020 support since version 72
- Safari: full ES2020 support since version 14
- Edge: full ES2020 support since version 80

## Explicitly Excluded Browsers

The following browsers are **not supported** and will not receive testing, bug fixes, or compatibility workarounds:

| Browser      | Reason                                                        |
|--------------|---------------------------------------------------------------|
| IE11         | No support for WebGL2, Service Workers, ES2020, or CSS Grid   |
| Opera Mini   | No support for WebGL2, Service Workers, or IndexedDB          |
| UC Browser   | Incomplete WebGL2 support, inconsistent API implementations   |

## Validation Method

Browser support is verified through **manual testing** using the GPX-to-export journey checklist. This end-to-end workflow exercises all critical Web APIs:

1. **Import** - Upload a GPX file (tests IndexedDB for draft storage, ES2020+ for parsing logic)
2. **Edit** - Modify the route on the map (tests WebGL2 via MapLibre GL JS, CSS Grid/Flexbox for layout)
3. **Save** - Save the activity (tests Service Worker for caching, network requests)
4. **Export** - Export the result (tests full rendering pipeline)

This journey is executed on each supported browser/version combination before major releases. Automated browser testing may supplement manual testing in the future but is not currently in place.

### When to Re-validate

- On every major dependency upgrade (especially MapLibre GL JS)
- When adding new Web API dependencies
- Before each production release
- When browser vendors announce deprecations affecting supported APIs
