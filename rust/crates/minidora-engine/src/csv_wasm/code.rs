//! Encode the two closed integer aggregation algorithms as WebAssembly.

use super::{
    ARTIFACT_NAME, DESCRIPTOR_BYTES, HEADER_BYTES, MAX_GROUPS, MAX_LABEL_BYTES, MAX_PACKET_BYTES,
    MAX_ROWS, ROW_BYTES, ROWS_OFFSET,
};

const I32: u8 = 0x7f;
const I64: u8 = 0x7e;
const VOID: u8 = 0x40;
const BLOCK: u8 = 0x02;
const LOOP: u8 = 0x03;
const IF: u8 = 0x04;
const END: u8 = 0x0b;
const I32_EQZ: u8 = 0x45;
const I32_NE: u8 = 0x47;
const I32_LT_U: u8 = 0x49;
const I32_GT_U: u8 = 0x4b;
const I32_GE_U: u8 = 0x4f;
const I64_NE: u8 = 0x52;
const I64_LT_S: u8 = 0x53;
const I64_LT_U: u8 = 0x54;
const I32_ADD: u8 = 0x6a;
const I32_SUB: u8 = 0x6b;
const I32_MUL: u8 = 0x6c;
const I64_ADD: u8 = 0x7c;
const I64_SUB: u8 = 0x7d;
const I64_DIV_U: u8 = 0x80;
const I64_REM_U: u8 = 0x82;
const I64_SHR_S: u8 = 0x87;
const I32_WRAP_I64: u8 = 0xa7;
const I64_EXTEND_I32_U: u8 = 0xad;

const FD_READ: u32 = 0;
const FD_WRITE: u32 = 1;
const PATH_OPEN: u32 = 2;
const PROC_EXIT: u32 = 3;
const WRITE: u32 = 4;
const DECIMAL: u32 = 5;
const START: u32 = 6;

const IO_VECTOR: i32 = 0;
const IO_SIZE: i32 = 8;
const OUT_FD: i32 = 12;
const TOTALS: i32 = 256;
const TOTAL_BYTES: i32 = 16;
const TOTAL_HIGH: u32 = 8;
const FILE_NAME: i32 = 1024;
const SUCCESS: i32 = 1088;
const OVERFLOW: i32 = 1216;
const LINE_END: i32 = 1344;
const NUMBER_END: i32 = 1440;
const INPUT: i32 = 8192;
const _: () = assert!(TOTALS + MAX_GROUPS as i32 * TOTAL_BYTES <= FILE_NAME);

const SUCCESS_TEXT: &str = "集計が完了しました。summary.csv\n";
const OVERFLOW_TEXT: &str = "整数の合計が64ビットの範囲を超えました。\n";

fn unsigned(into: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        into.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    into.push(value as u8);
}

fn signed(into: &mut Vec<u8>, mut value: i64) {
    loop {
        let byte = value as u8 & 0x7f;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        into.push(if done { byte } else { byte | 0x80 });
        if done {
            break;
        }
    }
}

fn bytes(into: &mut Vec<u8>, value: &[u8]) {
    unsigned(into, value.len() as u32);
    into.extend_from_slice(value);
}

fn section(module: &mut Vec<u8>, id: u8, content: &[u8]) {
    module.push(id);
    bytes(module, content);
}

#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
    fn op(&mut self, opcode: u8) -> &mut Self {
        self.0.push(opcode);
        self
    }

    fn arg(&mut self, opcode: u8, argument: u32) -> &mut Self {
        self.op(opcode);
        unsigned(&mut self.0, argument);
        self
    }

    fn int(&mut self, value: i32) -> &mut Self {
        self.op(0x41);
        signed(&mut self.0, i64::from(value));
        self
    }

    fn wide(&mut self, value: i64) -> &mut Self {
        self.op(0x42);
        signed(&mut self.0, value);
        self
    }

    fn get(&mut self, index: u32) -> &mut Self {
        self.arg(0x20, index)
    }
    fn set(&mut self, index: u32) -> &mut Self {
        self.arg(0x21, index)
    }
    fn call(&mut self, index: u32) -> &mut Self {
        self.arg(0x10, index)
    }
    fn branch(&mut self, depth: u32) -> &mut Self {
        self.arg(0x0c, depth)
    }
    fn branch_if(&mut self, depth: u32) -> &mut Self {
        self.arg(0x0d, depth)
    }

    fn mem(&mut self, opcode: u8, offset: u32) -> &mut Self {
        self.arg(opcode, 0); // unaligned accesses are admitted
        unsigned(&mut self.0, offset);
        self
    }

    fn load(&mut self, offset: u32) -> &mut Self {
        self.mem(0x28, offset)
    }
    fn load_wide(&mut self, offset: u32) -> &mut Self {
        self.mem(0x29, offset)
    }
    fn store(&mut self, offset: u32) -> &mut Self {
        self.mem(0x36, offset)
    }
    fn store_wide(&mut self, offset: u32) -> &mut Self {
        self.mem(0x37, offset)
    }
    fn store_byte(&mut self) -> &mut Self {
        self.mem(0x3a, 0)
    }

    fn begin(&mut self, kind: u8) -> &mut Self {
        self.op(kind).op(VOID)
    }

    fn exit(&mut self, code: i32) -> &mut Self {
        self.int(code).call(PROC_EXIT).op(0x00)
    }

    // The caller leaves an error condition on the stack.
    fn reject_if(&mut self, code: i32) -> &mut Self {
        self.begin(IF).exit(code).op(END)
    }

    fn write_literal(&mut self, fd: i32, start: i32, value: &str) -> &mut Self {
        self.int(fd).int(start).int(value.len() as i32).call(WRITE)
    }

    // Take a group index from the stack and leave its 128-bit total's address.
    fn total_address(&mut self) -> &mut Self {
        self.int(TOTAL_BYTES).op(I32_MUL).int(TOTALS).op(I32_ADD)
    }

    fn finish(mut self, types: &[(u8, u32)]) -> Vec<u8> {
        let mut code = Vec::new();
        unsigned(&mut code, types.len() as u32);
        for (kind, count) in types {
            unsigned(&mut code, *count);
            code.push(*kind);
        }
        self.op(END);
        code.extend(self.0);
        code
    }
}

// (fd, pointer, size) -> ()
fn write_function() -> Vec<u8> {
    let mut body = Body::default();
    body.int(IO_VECTOR).get(1).store(0);
    body.int(IO_VECTOR).get(2).store(4);
    body.get(0)
        .int(IO_VECTOR)
        .int(1)
        .int(IO_SIZE)
        .call(FD_WRITE)
        .reject_if(5);
    body.int(IO_SIZE).load(0).get(2).op(I32_NE).reject_if(5);
    body.finish(&[])
}

// Format signed values by dividing the unsigned magnitude, including i64::MIN.
fn decimal_function() -> Vec<u8> {
    const FD: u32 = 0;
    const VALUE: u32 = 1;
    const POSITION: u32 = 2;
    const NEGATIVE: u32 = 3;
    let mut body = Body::default();
    body.get(VALUE).wide(0).op(I64_LT_S).set(NEGATIVE);
    body.get(NEGATIVE).begin(IF);
    body.wide(0).get(VALUE).op(I64_SUB).set(VALUE).op(END);
    body.int(NUMBER_END).set(POSITION);
    body.begin(LOOP);
    body.get(POSITION).int(1).op(I32_SUB).set(POSITION);
    body.get(POSITION)
        .get(VALUE)
        .wide(10)
        .op(I64_REM_U)
        .op(I32_WRAP_I64)
        .int(i32::from(b'0'))
        .op(I32_ADD)
        .store_byte();
    body.get(VALUE).wide(10).op(I64_DIV_U).set(VALUE);
    body.get(VALUE).wide(0).op(I64_NE).branch_if(0).op(END);
    body.get(NEGATIVE).begin(IF);
    body.get(POSITION).int(1).op(I32_SUB).set(POSITION);
    body.get(POSITION).int(i32::from(b'-')).store_byte().op(END);
    body.get(FD)
        .get(POSITION)
        .int(NUMBER_END)
        .get(POSITION)
        .op(I32_SUB)
        .call(WRITE);
    body.finish(&[(I32, 2)])
}

fn start_function(sum: bool) -> Vec<u8> {
    const READ: u32 = 0;
    const ROWS: u32 = 1;
    const GROUPS: u32 = 2;
    const TEXT: u32 = 3;
    const INDEX: u32 = 4;
    const RECORD: u32 = 5;
    const POINTER: u32 = 6;
    const FD: u32 = 7;
    const LABEL_OFFSET: u32 = 8;
    const LABEL_SIZE: u32 = 9;
    const BEFORE: u32 = 10;
    const AMOUNT: u32 = 11;
    const AFTER: u32 = 12;
    let mut body = Body::default();

    body.int(IO_VECTOR).int(INPUT).store(0);
    body.int(IO_VECTOR).int(MAX_PACKET_BYTES as i32).store(4);
    body.int(0)
        .int(IO_VECTOR)
        .int(1)
        .int(IO_SIZE)
        .call(FD_READ)
        .reject_if(3);
    body.int(IO_SIZE).load(0).set(READ);
    body.get(READ)
        .int(ROWS_OFFSET as i32)
        .op(I32_LT_U)
        .reject_if(3);
    body.int(INPUT).load(0).get(READ).op(I32_NE).reject_if(3);
    body.int(INPUT).load(4).set(ROWS);
    body.get(ROWS).op(I32_EQZ).reject_if(3);
    body.get(ROWS)
        .int(MAX_ROWS as i32)
        .op(I32_GT_U)
        .reject_if(3);
    body.int(INPUT).load(8).set(GROUPS);
    body.get(GROUPS).op(I32_EQZ).reject_if(3);
    body.get(GROUPS)
        .int(MAX_GROUPS as i32)
        .op(I32_GT_U)
        .reject_if(3);
    body.get(ROWS)
        .int(ROW_BYTES as i32)
        .op(I32_MUL)
        .int(ROWS_OFFSET as i32)
        .op(I32_ADD)
        .set(TEXT);
    body.int(INPUT)
        .load(12)
        .int((MAX_LABEL_BYTES * 2 + 32) as i32)
        .op(I32_GT_U)
        .reject_if(3);
    body.get(TEXT)
        .int(INPUT)
        .load(12)
        .op(I32_ADD)
        .get(READ)
        .op(I32_GT_U)
        .reject_if(3);

    // The rows are consumed in original order; accumulate only in guest memory.
    body.begin(BLOCK).begin(LOOP);
    body.get(INDEX).get(ROWS).op(I32_GE_U).branch_if(1);
    body.get(INDEX)
        .int(ROW_BYTES as i32)
        .op(I32_MUL)
        .int(INPUT + ROWS_OFFSET as i32)
        .op(I32_ADD)
        .set(RECORD);
    body.get(RECORD)
        .load(0)
        .get(GROUPS)
        .op(I32_GE_U)
        .reject_if(3);
    body.get(RECORD).load(0).total_address().set(POINTER);
    body.get(POINTER).load_wide(0).set(BEFORE);
    if sum {
        body.get(RECORD).load_wide(4);
    } else {
        body.wide(1);
    }
    body.set(AMOUNT);
    body.get(BEFORE).get(AMOUNT).op(I64_ADD).set(AFTER);
    // 4096 signed 64-bit inputs fit exactly in a signed 128-bit total.
    // Add the input's upper limb and the carry from the unsigned lower limb.
    body.get(POINTER)
        .get(POINTER)
        .load_wide(TOTAL_HIGH)
        .get(AMOUNT)
        .wide(63)
        .op(I64_SHR_S)
        .op(I64_ADD)
        .get(AFTER)
        .get(BEFORE)
        .op(I64_LT_U)
        .op(I64_EXTEND_I32_U)
        .op(I64_ADD)
        .store_wide(TOTAL_HIGH);
    body.get(POINTER).get(AFTER).store_wide(0);
    body.get(INDEX).int(1).op(I32_ADD).set(INDEX).branch(0);
    body.op(END).op(END);

    // A final total fits in i64 iff its upper limb equals its sign extension.
    body.int(0).set(INDEX);
    body.begin(BLOCK).begin(LOOP);
    body.get(INDEX).get(GROUPS).op(I32_GE_U).branch_if(1);
    body.get(INDEX).total_address().set(POINTER);
    body.get(POINTER)
        .load_wide(TOTAL_HIGH)
        .get(POINTER)
        .load_wide(0)
        .wide(63)
        .op(I64_SHR_S)
        .op(I64_NE)
        .begin(IF);
    body.write_literal(2, OVERFLOW, OVERFLOW_TEXT)
        .exit(2)
        .op(END);
    body.get(INDEX).int(1).op(I32_ADD).set(INDEX).branch(0);
    body.op(END).op(END);

    // Open the declared scratch file, and only after all arithmetic checks.
    body.int(3)
        .int(0)
        .int(FILE_NAME)
        .int(ARTIFACT_NAME.len() as i32)
        .int(1)
        .wide(1 << 6)
        .wide(0)
        .int(0)
        .int(OUT_FD)
        .call(PATH_OPEN)
        .reject_if(5);
    body.int(OUT_FD).load(0).set(FD);
    body.get(FD)
        .int(INPUT)
        .get(TEXT)
        .op(I32_ADD)
        .int(INPUT)
        .load(12)
        .call(WRITE);
    body.int(0).set(INDEX);
    body.begin(BLOCK).begin(LOOP);
    body.get(INDEX).get(GROUPS).op(I32_GE_U).branch_if(1);
    body.get(INDEX)
        .int(DESCRIPTOR_BYTES as i32)
        .op(I32_MUL)
        .int(INPUT + HEADER_BYTES as i32)
        .op(I32_ADD)
        .set(RECORD);
    body.get(RECORD).load(0).set(LABEL_OFFSET);
    body.get(RECORD).load(4).set(LABEL_SIZE);
    body.get(LABEL_OFFSET).get(READ).op(I32_GT_U).reject_if(3);
    body.get(LABEL_SIZE)
        .get(READ)
        .get(LABEL_OFFSET)
        .op(I32_SUB)
        .op(I32_GT_U)
        .reject_if(3);
    body.get(FD)
        .int(INPUT)
        .get(LABEL_OFFSET)
        .op(I32_ADD)
        .get(LABEL_SIZE)
        .call(WRITE);
    body.get(FD)
        .get(INDEX)
        .total_address()
        .load_wide(0)
        .call(DECIMAL);
    body.get(FD).int(LINE_END).int(1).call(WRITE);
    body.get(INDEX).int(1).op(I32_ADD).set(INDEX).branch(0);
    body.op(END).op(END);
    body.write_literal(1, SUCCESS, SUCCESS_TEXT);
    body.finish(&[(I32, 10), (I64, 3)])
}

pub(super) fn module(sum: bool) -> Vec<u8> {
    let mut module = b"\0asm\x01\0\0\0".to_vec();
    let mut types = vec![6];
    let signatures: &[(&[u8], &[u8])] = &[
        (&[I32; 4], &[I32]),
        (&[I32, I32, I32, I32, I32, I64, I64, I32, I32], &[I32]),
        (&[I32], &[]),
        (&[I32; 3], &[]),
        (&[I32, I64], &[]),
        (&[], &[]),
    ];
    for (parameters, results) in signatures {
        types.push(0x60);
        bytes(&mut types, parameters);
        bytes(&mut types, results);
    }
    section(&mut module, 1, &types);
    let mut imports = vec![4];
    for (name, type_index) in [
        ("fd_read", 0),
        ("fd_write", 0),
        ("path_open", 1),
        ("proc_exit", 2),
    ] {
        bytes(&mut imports, b"wasi_snapshot_preview1");
        bytes(&mut imports, name.as_bytes());
        imports.extend_from_slice(&[0, type_index]);
    }
    section(&mut module, 2, &imports);
    section(&mut module, 3, &[3, 3, 4, 5]); // three defined functions
    section(&mut module, 5, &[1, 1, 2, 2]); // one two-page memory, fixed upper bound
    let mut exports = vec![2];
    bytes(&mut exports, b"memory");
    exports.extend_from_slice(&[2, 0]);
    bytes(&mut exports, b"_start");
    exports.push(0);
    unsigned(&mut exports, START);
    section(&mut module, 7, &exports);

    let mut bodies = vec![3];
    for function in [write_function(), decimal_function(), start_function(sum)] {
        bytes(&mut bodies, &function);
    }
    section(&mut module, 10, &bodies);
    let literals = [
        (FILE_NAME, ARTIFACT_NAME),
        (SUCCESS, SUCCESS_TEXT),
        (OVERFLOW, OVERFLOW_TEXT),
        (LINE_END, "\n"),
    ];
    let mut data = vec![literals.len() as u8];
    for (address, text) in literals {
        data.extend_from_slice(&[0, 0x41]); // active segment in memory zero, then i32.const
        signed(&mut data, i64::from(address));
        data.push(END);
        bytes(&mut data, text.as_bytes());
    }
    section(&mut module, 11, &data);
    module
}
