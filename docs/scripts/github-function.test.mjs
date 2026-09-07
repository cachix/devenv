import assert from 'node:assert/strict';
import test from 'node:test';

import { onRequestGet } from '../../functions/api/github.js';

test('the GitHub metadata function resolves the public Site Kit export', () => {
  assert.equal(typeof onRequestGet, 'function');
});
