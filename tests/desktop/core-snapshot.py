"""Copy an explicitly selected v7 development database and referenced DPAPI blobs.

Uses a read-only source transaction; never decrypts credentials or copies live WALs.
The destination must not already contain a database.
"""
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
import sys
import uuid

source, target = (Path(p).resolve() for p in sys.argv[1:3])
assert source != target and source not in target.parents
target.mkdir(parents=True, exist_ok=True)
destination = target / 'core.sqlite'
assert not destination.exists(), 'Destination database must be new'
with sqlite3.connect((source / 'core.sqlite').as_uri() + '?mode=ro', uri=True) as original:
    original.execute('BEGIN')
    assert original.execute('PRAGMA user_version').fetchone()[0] == 7
    with sqlite3.connect(destination) as copied:
        original.backup(copied)
        assert copied.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        identifiers = [row[0] for row in copied.execute('SELECT secret_id FROM credential_bindings')]
        (target / 'secrets').mkdir(exist_ok=True)
        for identifier in identifiers:
            assert str(uuid.UUID(identifier)) == identifier
            filename = identifier + '.dpapi'
            shutil.copy2(source / 'secrets' / filename, target / 'secrets' / filename)
        tables = [row[0] for row in copied.execute("SELECT name FROM sqlite_master WHERE type='table'")]
        counts, digests = {}, {}
        for table in tables:
            rows = copied.execute('SELECT * FROM "' + table.replace('"', '""') + '"').fetchall()
            counts[table] = len(rows)
            digests[table] = hashlib.sha256(json.dumps(sorted(rows), ensure_ascii=False).encode()).hexdigest()
result = dict(source=str(source), target=str(target), schema=7, integrity='ok',
              method='Read-only SQLite online backup; referenced encrypted DPAPI blobs only',
              encryptedFiles=len(identifiers), tables=counts, rowDigests=digests)
print(json.dumps(result, ensure_ascii=False))
