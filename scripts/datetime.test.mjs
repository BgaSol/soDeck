import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import test from 'node:test';
import { date, localTime, parseLocalTime } from '../src/datetime.ts';

test('empty filters and invalid dates do not become timestamps', () => {
  assert.equal(localTime(), '');
  for (const input of ['', 'invalid', '2023-02-29T12:00']) {
    assert.equal(parseLocalTime(input), undefined);
  }
  assert.equal(date('invalid'), 'Invalid Date');
  assert.equal(date(NaN), 'Invalid Date');
});

const instants = [
  '1970-01-01T00:00:00Z',
  '2000-02-29T23:59:59.999Z',
  '2024-03-10T06:59:00Z',
  '2024-03-10T07:00:00Z',
  '2024-11-03T05:30:00Z',
  '2024-11-03T07:30:00Z',
];
const zones = {
  UTC: ['1970-01-01T00:00', '2000-02-29T23:59', '2024-03-10T06:59', '2024-03-10T07:00', '2024-11-03T05:30', '2024-11-03T07:30'],
  'Asia/Shanghai': ['1970-01-01T08:00', '2000-03-01T07:59', '2024-03-10T14:59', '2024-03-10T15:00', '2024-11-03T13:30', '2024-11-03T15:30'],
  'Asia/Kathmandu': ['1970-01-01T05:30', '2000-03-01T05:44', '2024-03-10T12:44', '2024-03-10T12:45', '2024-11-03T11:15', '2024-11-03T13:15'],
  'America/New_York': ['1969-12-31T19:00', '2000-02-29T18:59', '2024-03-10T01:59', '2024-03-10T03:00', '2024-11-03T01:30', '2024-11-03T02:30'],
};

for (const [timezone, expected] of Object.entries(zones)) {
  test(`date display and minute filters use local time in ${timezone}`, () => {
    const source = `
      import assert from 'node:assert/strict';
      import { date, localTime, parseLocalTime } from ${JSON.stringify(new URL('../src/datetime.ts', import.meta.url).href)};
      const instants = ${JSON.stringify(instants)}, expected = ${JSON.stringify(expected)};
      for (const [index, iso] of instants.entries()) {
        const millis = Date.parse(iso);
        assert.equal(localTime(millis), expected[index], iso);
        assert.equal(parseLocalTime(expected[index]), Math.floor(millis / 60000) * 60000, iso);
        const display = new Date(millis).toLocaleString('zh-CN', { hour12: false });
        assert.equal(date(millis), display, iso);
        assert.equal(date(iso), display, iso);
      }
      assert.equal(date('2024-02-29T08:00:00+08:00'), date(Date.parse('2024-02-29T00:00:00Z')));
      if (${JSON.stringify(timezone)} === 'America/New_York') {
        assert.equal(parseLocalTime('2024-03-10T02:30'), Date.parse('2024-03-10T07:30:00Z'));
        assert.equal(parseLocalTime('2024-11-03T01:30'), Date.parse('2024-11-03T05:30:00Z'));
      }
    `;
    execFileSync(process.execPath, ['--input-type=module', '-e', source], {
      env: { ...process.env, TZ: timezone }, windowsHide: true, stdio: 'pipe',
    });
  });
}
