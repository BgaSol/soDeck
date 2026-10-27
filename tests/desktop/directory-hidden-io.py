"""Audit file-plugin remote I/O during the native hidden-panel interval."""
import json
import pathlib
import sqlite3

root = pathlib.Path(__file__).resolve().parents[2]
result = json.loads((root / '.qa/t010/native/results.json').read_text(encoding='utf-8'))
database = pathlib.Path(result['data']) / 'core/core.sqlite'
with sqlite3.connect(f'file:{database.as_posix()}?mode=ro', uri=True) as connection:
    checks = []
    for interval in result['hiddenIntervals']:
        calls = connection.execute(
            'select caller, method from calls where started >= ? and started <= ?',
            (interval['start'], interval['end']),
        ).fetchall()
        remote = [method for caller, method in calls
                  if json.loads(caller).get('pluginId') == 'sodeck.files'
                  and (method.startswith('files.') or method.startswith('remote.'))]
        checks.append({'interval': interval, 'remoteFileCalls': len(remote)})
        if remote:
            raise AssertionError(f'File plugin made remote calls while hidden: {remote}')

(root / '.qa/t010/hidden-io.json').write_text(
    json.dumps(checks, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(json.dumps(checks))
