"use strict";

// Fixed host for a closed WASI preview1 subset. Guest code is always raw Wasm;
// no JavaScript, host filesystem, network, command, clock, or environment API
// is passed to it. The parent owns launch, deadline, and receipt readback.
const fs = require("node:fs");
const path = require("node:path");

const MAX_BYTES = 64 * 1024;
const MAX_FILES = 4;
const MAX_MEMORY_PAGES = 128;
const MAX_TABLE = 256;
const MAX_IOVECS = 1024;
const MAX_REQUEST_BYTES = 4 * MAX_BYTES + 1024;
const text = new TextDecoder("utf-8", { fatal: true });
const E = {
    ok: 0, badf: 8, exist: 20, fault: 21, inval: 28, noent: 44,
    nospc: 51, notcapable: 76,
};
const RIGHTS_READ = 1n << 1n;
const RIGHTS_WRITE = 1n << 6n;
const RIGHTS_PATH_CREATE = 1n << 10n;
const RIGHTS_PATH_OPEN = 1n << 13n;
const RIGHTS_PATH_STAT = 1n << 18n;
const RIGHTS_STAT = 1n << 21n;
const FILE_RIGHTS = RIGHTS_READ | RIGHTS_WRITE | RIGHTS_STAT | 0x3dn;
const FAULT = Symbol("invalid guest memory range");
const BAD_ABI = Symbol("invalid WASI argument");
const EXIT = Symbol("guest exit");

function safeName(name) {
    return typeof name === "string"
        && /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(name)
        && !name.endsWith(".")
        && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(name)
        && !/^receipt\.json$/i.test(name);
}

function decodeHex(value, allowEmpty) {
    if (typeof value !== "string" || value.length > 2 * MAX_BYTES
        || (!allowEmpty && value.length === 0) || value.length % 2 !== 0
        || !/^[0-9a-f]*$/.test(value)) {
        throw new Error("invalid byte payload");
    }
    return Buffer.from(value, "hex");
}

function request() {
    const buffer = Buffer.alloc(MAX_REQUEST_BYTES + 1);
    let used = 0;
    while (used < buffer.length) {
        const n = fs.readSync(0, buffer, used, buffer.length - used, null);
        if (n === 0) break;
        used += n;
    }
    if (used > MAX_REQUEST_BYTES) throw new Error("request budget exceeded");
    const value = JSON.parse(text.decode(buffer.subarray(0, used)));
    if (value === null || typeof value !== "object" || Array.isArray(value)
        || Object.keys(value).sort().join(",") !== "files,module_hex,stdin_hex"
        || !Array.isArray(value.files) || value.files.length > MAX_FILES
        || !value.files.every(safeName)
        || new Set(value.files.map(name => name.toLowerCase())).size !== value.files.length) {
        throw new Error("invalid request");
    }
    return {
        module: decodeHex(value.module_hex, false),
        stdin: decodeHex(value.stdin_hex, true),
        files: value.files,
    };
}

// Validate bounded memory/table shape before V8 compilation can allocate a
// guest instance. V8 also validates the complete binary and enforces its caps.
function validateLimits(bytes) {
    if (bytes.length < 8 || !bytes.subarray(0, 8).equals(Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]))) {
        throw new Error("invalid WebAssembly header");
    }
    let cursor = 8;
    let end = bytes.length;
    let hasMemory = false;
    let hasTable = false;
    function byte() {
        if (cursor >= end) throw new Error("truncated WebAssembly section");
        return bytes[cursor++];
    }
    function u32() {
        let value = 0;
        for (let shift = 0; shift <= 28; shift += 7) {
            const b = byte();
            if (shift === 28 && b > 15) throw new Error("oversize WebAssembly integer");
            value += (b & 127) * 2 ** shift;
            if (b < 128) return value;
        }
        throw new Error("invalid WebAssembly integer");
    }
    function bounded(maximum) {
        if (byte() !== 1) throw new Error("an explicit non-shared 32-bit limit is required");
        const low = u32();
        const high = u32();
        if (low > high || high > maximum) throw new Error("WebAssembly limit exceeds the runtime budget");
    }
    while (cursor < bytes.length) {
        const section = byte();
        const length = u32();
        const next = cursor + length;
        if (next > bytes.length) throw new Error("invalid WebAssembly section length");
        end = next;
        if (section === 4) {
            if (hasTable || u32() !== 1 || byte() !== 0x70) {
                throw new Error("at most one bounded funcref table is supported");
            }
            bounded(MAX_TABLE);
            hasTable = true;
        } else if (section === 5) {
            if (hasMemory || u32() !== 1) throw new Error("exactly one guest memory is required");
            bounded(MAX_MEMORY_PAGES);
            hasMemory = true;
        } else if (section === 8) {
            throw new Error("use the WASI _start export");
        } else {
            cursor = next;
        }
        if (cursor !== next) throw new Error("invalid WebAssembly limits section");
        end = bytes.length;
    }
    if (!hasMemory) throw new Error("exactly one guest memory is required");
}

function execute(input) {
    validateLimits(input.module);
    const files = new Map();
    const handles = new Map();
    const open = new Set([0, 1, 2, 3]);
    const allowed = new Set(input.files);
    const streamUsed = [0, 0, 0];
    let inputPosition = 0;
    let memory;
    let guestExit = null;
    let exhausted = false;

    function overBudget() {
        exhausted = true;
        throw BAD_ABI;
    }

    function u32(value) {
        if (typeof value !== "number" || !Number.isInteger(value)
            || value < -2147483648 || value > 0xffffffff) throw BAD_ABI;
        return value >>> 0;
    }
    function data(ptr, size) {
        const offset = u32(ptr);
        const length = u32(size);
        if (offset + length > memory.buffer.byteLength) throw FAULT;
        return new Uint8Array(memory.buffer, offset, length);
    }
    function view(ptr, size) {
        const bytes = data(ptr, size);
        return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    }
    function utf8(ptr, size) {
        if (u32(size) > 64) return null;
        try { return text.decode(data(ptr, size)); }
        catch (error) {
            if (error === FAULT || error === BAD_ABI) throw error;
            return null;
        }
    }
    function vectors(ptr, count) {
        const n = u32(count);
        if (n > MAX_IOVECS) throw BAD_ABI;
        const table = view(ptr, 8 * n);
        const buffers = [];
        for (let i = 0; i < n; i++) {
            buffers.push(data(table.getUint32(8 * i, true), table.getUint32(8 * i + 4, true)));
        }
        return buffers;
    }
    function handle(fd, rights) {
        const file = handles.get(fd);
        return file && (file.rights & rights) === rights ? file : null;
    }
    function stat(ptr, kind, byteLength) {
        const bytes = data(ptr, 64);
        bytes.fill(0);
        bytes[16] = kind;
        const result = view(ptr, 64);
        result.setBigUint64(24, 1n, true);
        result.setBigUint64(32, BigInt(byteLength), true);
        return E.ok;
    }

    // Unlike a generic WASI shim, every name has a deliberately bounded local
    // behavior. Unlisted imports cause refusal before any guest instruction.
    const calls = {
        fd_read(fd, iovs, count, out) {
            const f = handle(fd, RIGHTS_READ);
            if (!(fd === 0 && open.has(fd)) && !f) return E.badf;
            const result = view(out, 4);
            const buffers = vectors(iovs, count);
            const length = f ? f.file.length : input.stdin.length;
            const source = f ? f.file.bytes : input.stdin;
            let pos = f ? f.position : inputPosition;
            let used = 0;
            for (const buffer of buffers) {
                const n = Math.max(0, Math.min(buffer.length, length - pos));
                buffer.set(source.subarray(pos, pos + n));
                pos += n;
                used += n;
            }
            if (f) f.position = pos;
            else inputPosition = pos;
            result.setUint32(0, used, true);
            return E.ok;
        },
        fd_write(fd, iovs, count, out) {
            const stdio = (fd === 1 || fd === 2) && open.has(fd);
            const f = handle(fd, RIGHTS_WRITE);
            if (!stdio && !f) return E.badf;
            const result = view(out, 4);
            const buffers = vectors(iovs, count);
            const n = buffers.reduce((sum, buffer) => sum + buffer.length, 0);
            if (stdio) {
                if (streamUsed[fd] + n > MAX_BYTES) overBudget();
                for (const buffer of buffers) {
                    for (let offset = 0; offset < buffer.length;) {
                        const wrote = fs.writeSync(fd, buffer, offset, buffer.length - offset);
                        if (wrote <= 0) throw new Error("stdio write did not progress");
                        offset += wrote;
                    }
                }
                streamUsed[fd] += n;
            } else {
                const file = f.file;
                const pos = f.append ? file.length : f.position;
                if (pos + n > MAX_BYTES || file.written + n > MAX_BYTES) overBudget();
                let offset = pos;
                for (const buffer of buffers) {
                    file.bytes.set(buffer, offset);
                    offset += buffer.length;
                }
                f.position = offset;
                file.length = Math.max(file.length, offset);
                file.written += n;
            }
            result.setUint32(0, n, true);
            return E.ok;
        },
        fd_close(fd) {
            if (open.delete(fd) || handles.delete(fd)) return E.ok;
            return E.badf;
        },
        fd_prestat_get(fd, out) {
            if (fd !== 3 || !open.has(3)) return E.badf;
            data(out, 8).fill(0);
            view(out, 8).setUint32(4, 1, true);
            return E.ok;
        },
        fd_prestat_dir_name(fd, out, len) {
            if (fd !== 3 || !open.has(3)) return E.badf;
            if (u32(len) < 1) return E.inval;
            data(out, 1)[0] = 46;
            return E.ok;
        },
        fd_fdstat_get(fd, out) {
            const f = handles.get(fd);
            if (!f && !open.has(fd)) return E.badf;
            data(out, 24).fill(0);
            const result = view(out, 24);
            result.setUint8(0, f ? 4 : fd === 3 ? 3 : 2);
            result.setUint16(2, f?.append ? 1 : 0, true);
            const rights = f ? f.rights : fd === 3
                ? RIGHTS_PATH_CREATE | RIGHTS_PATH_OPEN | RIGHTS_PATH_STAT | RIGHTS_STAT
                : fd === 0 ? RIGHTS_READ | RIGHTS_STAT : RIGHTS_WRITE | RIGHTS_STAT;
            result.setBigUint64(8, rights, true);
            result.setBigUint64(16, fd === 3 ? FILE_RIGHTS : 0n, true);
            return E.ok;
        },
        fd_filestat_get(fd, out) {
            const f = handles.get(fd);
            if (!f && !open.has(fd)) return E.badf;
            if (f && !handle(fd, RIGHTS_STAT)) return E.notcapable;
            return stat(out, f ? 4 : fd === 3 ? 3 : 2, f?.file.length || 0);
        },
        path_filestat_get(fd, flags, ptr, len, out) {
            if (fd !== 3 || !open.has(3)) return E.badf;
            if ((u32(flags) & ~1) !== 0) return E.inval;
            const name = utf8(ptr, len);
            if (!allowed.has(name)) return E.notcapable;
            const file = files.get(name);
            if (!file) return E.noent;
            return stat(out, 4, file.length);
        },
        path_open(fd, lookup, ptr, len, flags, rights, inheriting, fdflags, out) {
            if (fd !== 3 || !open.has(3)) return E.badf;
            if ((u32(lookup) & ~1) !== 0 || (u32(flags) & ~13) !== 0
                || (u32(fdflags) & ~1) !== 0) return E.inval;
            if (typeof rights !== "bigint" || typeof inheriting !== "bigint") throw BAD_ABI;
            if (rights < 0n || (rights & ~FILE_RIGHTS) !== 0n || inheriting !== 0n) return E.notcapable;
            const name = utf8(ptr, len);
            if (!allowed.has(name)) return E.notcapable;
            const result = view(out, 4);
            let file = files.get(name);
            if (file && (flags & 5) === 5) return E.exist;
            if (!file && (flags & 1) === 0) return E.noent;
            if ((!file || (flags & 8) !== 0) && (rights & RIGHTS_WRITE) === 0n) return E.notcapable;
            let next = 4;
            while (next < 4 + MAX_FILES && handles.has(next)) next++;
            if (next === 4 + MAX_FILES) return E.nospc;
            if (!file) {
                file = { bytes: Buffer.alloc(MAX_BYTES), length: 0, written: 0 };
                files.set(name, file);
            }
            if ((flags & 8) !== 0) {
                file.bytes.fill(0, 0, file.length);
                file.length = 0;
            }
            handles.set(next, { file, rights, position: 0, append: (fdflags & 1) !== 0 });
            result.setUint32(0, next, true);
            return E.ok;
        },
        fd_seek(fd, amount, whence, out) {
            const f = handle(fd, 1n << 2n);
            if (!f) return E.badf;
            if (typeof amount !== "bigint") throw BAD_ABI;
            const origin = whence === 0 ? 0 : whence === 1 ? f.position : whence === 2 ? f.file.length : null;
            if (origin === null) return E.inval;
            const pos = BigInt(origin) + amount;
            if (pos < 0n || pos > BigInt(f.file.length)) return E.inval;
            view(out, 8).setBigUint64(0, pos, true);
            f.position = Number(pos);
            return E.ok;
        },
        fd_tell(fd, out) {
            const f = handle(fd, 1n << 5n);
            if (!f) return E.badf;
            view(out, 8).setBigUint64(0, BigInt(f.position), true);
            return E.ok;
        },
        fd_fdstat_set_flags(fd, flags) {
            const f = handle(fd, 1n << 3n);
            if (!f) return E.badf;
            if ((u32(flags) & ~1) !== 0) return E.inval;
            f.append = (flags & 1) !== 0;
            return E.ok;
        },
        fd_sync(fd) {
            return handle(fd, 1n << 4n) ? E.ok : E.badf;
        },
        fd_datasync(fd) {
            return handle(fd, 1n) ? E.ok : E.badf;
        },
        args_sizes_get(count, size) {
            view(count, 4).setUint32(0, 0, true);
            view(size, 4).setUint32(0, 0, true);
            return E.ok;
        },
        args_get() { return E.ok; },
        environ_sizes_get(count, size) {
            return calls.args_sizes_get(count, size);
        },
        environ_get() { return E.ok; },
        proc_exit(status) {
            const code = u32(status);
            if (code > 255) throw BAD_ABI;
            guestExit = code;
            throw EXIT;
        },
    };
    const module = new WebAssembly.Module(input.module);
    const io = Object.create(null);
    for (const item of WebAssembly.Module.imports(module)) {
        if (item.kind !== "function" || item.module !== "wasi_snapshot_preview1"
            || !Object.hasOwn(calls, item.name)) throw new Error("unsupported WebAssembly import");
        const call = calls[item.name];
        io[item.name] = (...args) => {
            if (exhausted) throw BAD_ABI;
            if (guestExit !== null) throw EXIT;
            try { return call(...args); }
            catch (error) {
                if (error === FAULT) return E.fault;
                throw error;
            }
        };
    }
    const instance = new WebAssembly.Instance(module, { wasi_snapshot_preview1: io });
    const start = instance.exports._start;
    memory = instance.exports.memory;
    if (!(memory instanceof WebAssembly.Memory) || typeof start !== "function" || start.length !== 0) {
        throw new Error("missing WASI _start or memory export");
    }
    let status = "EXITED";
    try {
        if (start() !== undefined || exhausted) throw BAD_ABI;
        if (guestExit === null) guestExit = 0;
    } catch (error) {
        if (error !== EXIT) {
            status = "TRAP";
            guestExit = null;
        }
    }
    return { status, guestExit, files };
}

const scratch = require("node:worker_threads").workerData ?? process.argv[1];
if (typeof scratch !== "string" || !path.isAbsolute(scratch)) {
    process.exitCode = 3;
} else {
    let result;
    try {
        result = execute(request());
    } catch {
        result = { status: "REFUSED", guestExit: null, files: new Map() };
    }
    try {
        const names = [];
        for (const [name, file] of result.files) {
            fs.writeFileSync(path.join(scratch, name), file.bytes.subarray(0, file.length), { flag: "wx" });
            names.push(name);
        }
        fs.writeFileSync(path.join(scratch, "receipt.json"), JSON.stringify({
            status: result.status, guest_exit_code: result.guestExit, files: names,
        }), { flag: "wx" });
        process.exitCode = result.guestExit ?? (result.status === "TRAP" ? 1 : 2);
    } catch {
        process.exitCode = 3;
    }
}
