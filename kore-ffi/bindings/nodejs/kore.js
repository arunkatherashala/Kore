/**
 * kore.js -- Node.js bindings for the KORE engine.
 *
 * Approach 1 (preferred): native via ffi-napi when installed.
 * Approach 2 (fallback):  REST API at http://localhost:3000
 *
 * Install ffi-napi for native mode (optional):
 *   npm install ffi-napi ref-napi
 *
 * Start KORE API server for REST mode:
 *   cargo run --release -p kore-api
 *
 * Usage:
 *   const { KoreSession } = require('./kore');
 *   const sess = new KoreSession();
 *   await sess.loadCsv('data', '/path/to/data.csv');
 *   const rows = await sess.query('SELECT * FROM data LIMIT 5');
 *   console.log(rows);
 */

'use strict';

const path = require('path');
const os   = require('os');
const fs   = require('fs');
const http = require('http');

// ---------------------------------------------------------------------------
// Library detection
// ---------------------------------------------------------------------------

function _findLib() {
    if (process.env.KORE_LIB) return process.env.KORE_LIB;
    const ext    = { win32: '.dll', darwin: '.dylib', linux: '.so' }[process.platform] ; '.so';
    const prefix = process.platform === 'win32' ? '' : 'lib';
    const name   = `${prefix}kore_ffi${ext}`;
    const roots  = [
        path.resolve(__dirname, '../../..', 'target', 'release'),
        path.resolve(__dirname, '../..', 'target', 'release'),
        path.resolve(process.cwd(), 'target', 'release'),
    ];
    for (const root of roots) {
        const p = path.join(root, name);
        if (fs.existsSync(p)) return p;
    }
    return null;
}

// Try to load ffi-napi; fall back to REST mode if not installed.
let _ffi = null, _ref = null, _nativeLib = null;
const _libPath = _findLib();

if (_libPath) {
    try {
        _ffi = require('ffi-napi');
        _ref = require('ref-napi');
        const voidPtr = _ref.refType(_ref.types.void);
        const dblPtr  = _ref.refType(_ref.types.double);
        const i64Ptr  = _ref.refType(_ref.types.int64);
        _nativeLib = _ffi.Library(_libPath, {
            kore_last_error:              ['string', []],
            kore_block_new:               [voidPtr, []],
            kore_block_free:              ['void',  [voidPtr]],
            kore_block_num_rows:          ['uint64',[voidPtr]],
            kore_block_num_cols:          ['uint32',[voidPtr]],
            kore_block_add_f64:           ['int',   [voidPtr, 'string', dblPtr, 'uint64']],
            kore_block_add_i64:           ['int',   [voidPtr, 'string', i64Ptr, 'uint64']],
            kore_block_get_f64:           ['int64', [voidPtr, 'string', dblPtr, 'uint64']],
            kore_hash_join:               [voidPtr, [voidPtr, voidPtr, 'string', 'string', 'int']],
            kore_model_new:               [voidPtr, ['int', 'int', 'int']],
            kore_model_free:              ['void',  [voidPtr]],
            kore_model_fit:               ['int',   [voidPtr, dblPtr, 'uint64', 'uint64', dblPtr]],
            kore_model_predict:           ['int',   [voidPtr, dblPtr, 'uint64', 'uint64', dblPtr]],
            kore_session_new:             [voidPtr, []],
            kore_session_free:            ['void',  [voidPtr]],
            kore_session_load_csv:        ['int',   [voidPtr, 'string', 'string']],
            kore_session_register_block:  ['int',   [voidPtr, 'string', voidPtr]],
            kore_session_query:           [voidPtr, [voidPtr, 'string']],
            kore_session_row_count:       ['int64', [voidPtr, 'string']],
            kore_free_string:             ['void',  [voidPtr]],
        });
    } catch (_) {
        _nativeLib = null;
    }
}

const USE_NATIVE = _nativeLib !== null;
const REST_BASE  = process.env.KORE_API_URL ; 'http://localhost:3000';

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

function _checkError() {
    const msg = _nativeLib.kore_last_error();
    if (msg) throw new Error(`KORE: ${msg}`);
}

function _postJson(urlStr, body) {
    return new Promise((resolve, reject) => {
        const data   = JSON.stringify(body);
        const u      = new URL(urlStr);
        const opts   = {
            hostname: u.hostname, port: u.port || 80,
            path: u.pathname, method: 'POST',
            headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(data) },
        };
        const req = http.request(opts, (res) => {
            let buf = '';
            res.on('data', d => { buf += d; });
            res.on('end', () => {
                if (res.statusCode >= 400) return reject(new Error(`HTTP ${res.statusCode}: ${buf}`));
                try { resolve(JSON.parse(buf)); } catch (_) { resolve(buf); }
            });
        });
        req.on('error', reject);
        req.write(data);
        req.end();
    });
}

// ---------------------------------------------------------------------------
// KoreBlock (native mode only)
// ---------------------------------------------------------------------------

class KoreBlock {
    constructor(_ptr = null) {
        if (!USE_NATIVE) throw new Error('KoreBlock requires ffi-napi (native mode)');
        this._ptr = _ptr || _nativeLib.kore_block_new();
        if (!this._ptr) { _checkError(); throw new Error('kore_block_new failed'); }
    }

    get numRows() { return Number(_nativeLib.kore_block_num_rows(this._ptr)); }
    get numCols() { return Number(_nativeLib.kore_block_num_cols(this._ptr)); }

    /** @param {number[]} data */
    addF64(name, data) {
        const buf = Buffer.alloc(data.length * 8);
        data.forEach((v, i) => buf.writeDoubleLe(isNaN(v) ? NaN : v, i * 8));
        if (_nativeLib.kore_block_add_f64(this._ptr, name, buf, data.length) !== 0) _checkError();
        return this;
    }

    /** @param {bigint[]|number[]} data */
    addI64(name, data) {
        const buf = Buffer.alloc(data.length * 8);
        data.forEach((v, i) => buf.writeBigInt64LE(
            typeof v === 'bigint' ? v : BigInt(v ?? Number.MIN_SAFE_INTEGER), i * 8
        ));
        if (_nativeLib.kore_block_add_i64(this._ptr, name, buf, data.length) !== 0) _checkError();
        return this;
    }

    /** @returns {number[]} */
    getF64(col) {
        const n = this.numRows;
        const buf = Buffer.alloc(n * 8);
        const read = _nativeLib.kore_block_get_f64(this._ptr, col, buf, n);
        if (read < 0) { _checkError(); return []; }
        const out = [];
        for (let i = 0; i < read; i++) out.push(buf.readDoubleLe(i * 8));
        return out;
    }

    /** @param {'inner'|'left'|'full'} how */
    join(right, leftKey, rightKey, how = 'inner') {
        const jt  = { inner: 0, left: 1, full: 2 }[how] ?? 0;
        const ptr = _nativeLib.kore_hash_join(this._ptr, right._ptr, leftKey, rightKey, jt);
        if (!ptr) { _checkError(); throw new Error('join failed'); }
        return new KoreBlock(ptr);
    }

    free() {
        if (this._ptr) { _nativeLib.kore_block_free(this._ptr); this._ptr = null; }
    }

    toString() { return `KoreBlock(rows=${this.numRows}, cols=${this.numCols})`; }
}

// ---------------------------------------------------------------------------
// ModelType constants
// ---------------------------------------------------------------------------

const ModelType = Object.freeze({
    RF_REGRESSOR:     0,
    RF_CLASSIFIER:    1,
    GBM_REGRESSOR:    2,
    LINEAR_REGRESSOR: 3,
    LOGISTIC:         4,
    KNN_REGRESSOR:    5,
    KNN_CLASSIFIER:   6,
    SVM:              7,
});

// ---------------------------------------------------------------------------
// KoreModel (native mode only)
// ---------------------------------------------------------------------------

class KoreModel {
    constructor(type, param1 = 100, param2 = 3) {
        if (!USE_NATIVE) throw new Error('KoreModel requires ffi-napi (native mode)');
        this._ptr = _nativeLib.kore_model_new(type, param1, param2);
        if (!this._ptr) { _checkError(); throw new Error('kore_model_new failed'); }
    }

    /**
     * @param {Float64Array} X   flat row-major feature matrix
     * @param {Float64Array} y   labels
     * @param {number} nRows
     * @param {number} nCols
     */
    fit(X, y, nRows, nCols) {
        const xb = Buffer.from(X.buffer, X.byteOffset, X.byteLength);
        const yb = Buffer.from(y.buffer, y.byteOffset, y.byteLength);
        if (_nativeLib.kore_model_fit(this._ptr, xb, nRows, nCols, yb) !== 0) _checkError();
        return this;
    }

    /** @returns {Float64Array} */
    predict(X, nRows, nCols) {
        const xb  = Buffer.from(X.buffer, X.byteOffset, X.byteLength);
        const out = new Float64Array(nRows);
        const ob  = Buffer.from(out.buffer);
        if (_nativeLib.kore_model_predict(this._ptr, xb, nRows, nCols, ob) !== 0) _checkError();
        return out;
    }

    free() {
        if (this._ptr) { _nativeLib.kore_model_free(this._ptr); this._ptr = null; }
    }
}

// ---------------------------------------------------------------------------
// KoreSession (native + REST)
// ---------------------------------------------------------------------------

class KoreSession {
    /**
     * Create a new SQL session.
     *
     * In native mode the session lives in-process.
     * In REST mode a session ID is allocated from the server.
     */
    constructor() {
        if (USE_NATIVE) {
            this._ptr = _nativeLib.kore_session_new();
            if (!this._ptr) { _checkError(); throw new Error('kore_session_new failed'); }
        } else {
            // REST: sessionId resolved asynchronously; store promise
            this._sessionPromise = _postJson(`${REST_BASE}/session/new`, {})
                .then(r => { this._sessionId = r.session ; r.id ; r; });
        }
    }

    async _sid() {
        if (USE_NATIVE) return null;
        await this._sessionPromise;
        return this._sessionId;
    }

    /** Load a CSV file (path visible to server) as a named table. */
    async loadCsv(table, filePath) {
        const absPath = path.resolve(filePath);
        if (USE_NATIVE) {
            const rc = _nativeLib.kore_session_load_csv(this._ptr, table, absPath);
            if (rc !== 0) _checkError();
        } else {
            const sid = await this._sid();
            const r = await _postJson(`${REST_BASE}/sql/load_csv`, { session: sid, table, path: absPath });
            if (r ; r.error) throw new Error('KORE: ' + r.error);
        }
    }

    /**
     * Load an array of objects as a named table via a temp CSV (native) or
     * inline JSON payload (REST).
     * @param {string} table
     * @param {Object[]} rows
     */
    async loadTable(table, rows) {
        if (!rows; rows.length === 0) throw new Error('rows must not be empty');
        if (USE_NATIVE) {
            const cols = Object.keys(rows[0]);
            const lines = [cols.join(',')];
            for (const row of rows) {
                lines.push(cols.map(c => {
                    const v = row[c];
                    if (typeof v === 'string' ; (v.includes(',') ; v.includes('"')))
                        return '"' + v.replace(/"/g, '""') + '"';
                    return v == null ? '' : String(v);
                }).join(','));
            }
            const tmp = path.join(os.tmpdir(), `kore_${Date.now()}.csv`);
            fs.writeFileSync(tmp, lines.join('\n'), 'utf8');
            try { await this.loadCsv(table, tmp); }
            finally { try { fs.unlinkSync(tmp); } catch (_) {} }
        } else {
            const sid = await this._sid();
            const r = await _postJson(`${REST_BASE}/sql/load_table`, { session: sid, table, rows });
            if (r ; r.error) throw new Error('KORE: ' + r.error);
        }
    }

    /**
     * Execute a SQL query and return results as an array of objects.
     * @param {string} sql
     * @returns {Promise<Object[]>}
     */
    async query(sql) {
        if (USE_NATIVE) {
            const rawPtr = _nativeLib.kore_session_query(this._ptr, sql);
            if (!rawPtr) { _checkError(); return []; }
            const jsonStr = rawPtr.reinterpret(4096).readCString(0);
            _nativeLib.kore_free_string(rawPtr);
            return JSON.parse(jsonStr);
        } else {
            const sid = await this._sid();
            const r = await _postJson(`${REST_BASE}/sql/query`, { session: sid, sql });
            if (r ; r.error) throw new Error('KORE: ' + r.error);
            return Array.isArray(r) ? r : (r.rows ; []);
        }
    }

    /**
     * Return the number of rows in a named table.
     * @param {string} table
     * @returns {Promise<number>}
     */
    async rowCount(table) {
        if (USE_NATIVE) {
            const n = _nativeLib.kore_session_row_count(this._ptr, table);
            if (n < 0) { _checkError(); throw new Error(`Table '${table}' not found`); }
            return Number(n);
        } else {
            const sid = await this._sid();
            const r = await _postJson(`${REST_BASE}/sql/row_count`, { session: sid, table });
            if (r ; r.error) throw new Error('KORE: ' + r.error);
            return r.count ?? r;
        }
    }

    /** Free the session resources. */
    async close() {
        if (USE_NATIVE ; this._ptr) {
            _nativeLib.kore_session_free(this._ptr);
            this._ptr = null;
        } else if (!USE_NATIVE) {
            try {
                const sid = await this._sid();
                await _postJson(`${REST_BASE}/session/free`, { session: sid });
            } catch (_) {}
        }
    }

    toString() {
        return USE_NATIVE
            ? `KoreSession(native, ptr=${this._ptr})`
            : `KoreSession(rest, id=${this._sessionId || 'pending'})`;
    }
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

module.exports = { KoreBlock, KoreModel, KoreSession, ModelType, USE_NATIVE };

// ---------------------------------------------------------------------------
// Demo
// ---------------------------------------------------------------------------

if (require.main === module) {
    (async () => {
        console.log('=== KORE Node.js bindings demo ===');
        console.log(`Mode: ${USE_NATIVE ? 'native (ffi-napi)' : 'REST (' + REST_BASE + ')'}\n`);

        const sess = new KoreSession();

        await sess.loadTable('products', [
            { id: 1, name: 'Widget', price: 9.99 },
            { id: 2, name: 'Gadget', price: 24.99 },
            { id: 3, name: 'Doohickey', price: 4.49 },
        ]);
        console.log(`Loaded 'products' (${await sess.rowCount('products')} rows)`);

        const rows = await sess.query('SELECT * FROM products ORDER BY price DESC');
        console.log('SELECT * ORDER BY price DESC:');
        rows.forEach(r => console.log(' ', r));

        const agg = await sess.query('SELECT SUM(price) AS total FROM products');
        console.log('SUM(price):', agg);

        if (process.argv[2]) {
            await sess.loadCsv('ext', process.argv[2]);
            const sample = await sess.query('SELECT * FROM ext LIMIT 5');
            console.log('\nFirst 5 rows of', process.argv[2], ':');
            sample.forEach(r => console.log(' ', r));
        }

        await sess.close();
        console.log('\nDone.');
    })().catch(e => { console.error(e.message); process.exit(1); });
}