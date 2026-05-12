/**
 * KORE JavaScript Tests
 * Minimal test suite to verify bindings load correctly
 */

describe('KORE JavaScript Bindings', () => {
  test('should load successfully', () => {
    let moduleLoaded = false;
    try {
      const kore = require('./index.js');
      moduleLoaded = !!kore;
    } catch (err) {
      console.error('Failed to load kore module:', err.message);
    }
    expect(moduleLoaded).toBe(true);
  });

  test('should have required exports', () => {
    const kore = require('./index.js');
    expect(typeof kore).toBe('object');
  });
});

