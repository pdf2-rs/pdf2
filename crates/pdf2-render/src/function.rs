//! `/Function` objects (`7.10`): the one-input, many-output mappings a shading,
//! a separation or a stitching function is built from.
//!
//! Four of the five types are read — **0** sampled, **2** exponential, **3**
//! stitching and **4** the PostScript calculator. Type 1 is the PDF 1.0 sampled
//! form the standard deprecates and no file in the corpus uses, so it is refused
//! by name rather than aliased onto type 0. The calculator is a small stack
//! machine with the operator set of Table 42 and a bounded stack, because a
//! function is data from an untrusted file.
//!
//! Nothing paints with these yet — the `sh` operator that consumes them is the
//! next round's child — so the module is unused outside its tests for now, which
//! is what the `dead_code` allowance below says.
#![allow(dead_code)]

use pdf2::{Dictionary, Document, Object, Stream};

use crate::{Error, Result};

/// A parsed `/Function`.
#[derive(Debug, Clone)]
pub(crate) struct Function {
    domain: Vec<(f64, f64)>,
    range: Vec<(f64, f64)>,
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    /// Type 0: a table of samples, interpolated between.
    Sampled {
        sizes: Vec<usize>,
        bits: u8,
        encode: Vec<f64>,
        decode: Vec<f64>,
        samples: Vec<u32>,
    },
    /// Type 2: `C0 + x^N (C1 - C0)`.
    Exponential {
        c0: Vec<f64>,
        c1: Vec<f64>,
        exponent: f64,
    },
    /// Type 3: a sub-function chosen by `/Bounds`.
    Stitching {
        functions: Vec<Function>,
        bounds: Vec<f64>,
        encode: Vec<f64>,
    },
    /// Type 4: a PostScript calculator program.
    Calculator { program: Vec<Token> },
}

/// One token of a calculator program.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Operator(Op),
    Proc(Vec<Token>),
}

/// The calculator's operators (Table 42).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Abs,
    Add,
    Atan,
    BitShift,
    Ceiling,
    Copy,
    Cos,
    Cvi,
    Cvr,
    Div,
    Dup,
    Eq,
    Exch,
    Exp,
    False,
    Floor,
    Ge,
    Gt,
    Idiv,
    If,
    Ifelse,
    Index,
    Le,
    Ln,
    Log,
    Lt,
    Mod,
    Mul,
    Ne,
    Neg,
    Not,
    Or,
    Pop,
    Roll,
    Round,
    Sin,
    Sqrt,
    Sub,
    True,
    Truncate,
    Xor,
    And,
}

impl Op {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "abs" => Self::Abs,
            "add" => Self::Add,
            "atan" => Self::Atan,
            "bitshift" => Self::BitShift,
            "ceiling" => Self::Ceiling,
            "copy" => Self::Copy,
            "cos" => Self::Cos,
            "cvi" => Self::Cvi,
            "cvr" => Self::Cvr,
            "div" => Self::Div,
            "dup" => Self::Dup,
            "eq" => Self::Eq,
            "exch" => Self::Exch,
            "exp" => Self::Exp,
            "false" => Self::False,
            "floor" => Self::Floor,
            "ge" => Self::Ge,
            "gt" => Self::Gt,
            "idiv" => Self::Idiv,
            "if" => Self::If,
            "ifelse" => Self::Ifelse,
            "index" => Self::Index,
            "le" => Self::Le,
            "ln" => Self::Ln,
            "log" => Self::Log,
            "lt" => Self::Lt,
            "mod" => Self::Mod,
            "mul" => Self::Mul,
            "ne" => Self::Ne,
            "neg" => Self::Neg,
            "not" => Self::Not,
            "or" => Self::Or,
            "pop" => Self::Pop,
            "roll" => Self::Roll,
            "round" => Self::Round,
            "sin" => Self::Sin,
            "sqrt" => Self::Sqrt,
            "sub" => Self::Sub,
            "true" => Self::True,
            "truncate" => Self::Truncate,
            "xor" => Self::Xor,
            "and" => Self::And,
            _ => return None,
        })
    }
}

/// The most operators one evaluation may run, so a looping program cannot hang.
const MAX_STEPS: usize = 4096;
/// The most values the calculator stack may hold.
const MAX_STACK: usize = 128;

impl Function {
    /// Read a `/Function` (a dictionary or a stream) and its `/FunctionType`.
    pub(crate) fn parse(document: &Document, object: &Object) -> Result<Self> {
        let object = match object {
            Object::Reference(id) => document.get_object(*id)?,
            other => other.clone(),
        };
        let (dict, stream) = match &object {
            Object::Dictionary(dict) => (dict.clone(), None),
            Object::Stream(stream) => (stream.dict().clone(), Some(stream.clone())),
            _ => return Err(Error::malformed("a /Function that is not a dictionary")),
        };
        let kind = match dict.get(b"FunctionType").and_then(Object::as_integer) {
            Some(0) => Kind::Sampled {
                sizes: integers(&dict, b"Size")?,
                bits: u8::try_from(
                    dict.get(b"BitsPerSample")
                        .and_then(Object::as_integer)
                        .unwrap_or(8),
                )
                .map_err(|_| Error::malformed("a /BitsPerSample outside a byte"))?,
                encode: reals(&dict, b"Encode")?,
                decode: reals(&dict, b"Decode")?,
                samples: unpack_samples(
                    document,
                    stream
                        .as_ref()
                        .ok_or(Error::malformed("a sampled function without a stream"))?,
                    u8::try_from(
                        dict.get(b"BitsPerSample")
                            .and_then(Object::as_integer)
                            .unwrap_or(8),
                    )
                    .unwrap_or(8),
                )?,
            },
            Some(2) => Kind::Exponential {
                c0: if dict.get(b"C0").is_some() {
                    reals(&dict, b"C0")?
                } else {
                    vec![0.0]
                },
                c1: if dict.get(b"C1").is_some() {
                    reals(&dict, b"C1")?
                } else {
                    vec![1.0]
                },
                exponent: dict
                    .get(b"N")
                    .and_then(Object::as_real)
                    .ok_or(Error::malformed("an exponential function without /N"))?,
            },
            Some(3) => {
                let functions = match dict.get(b"Functions").map(|value| match value {
                    Object::Reference(id) => document.get_object(*id),
                    other => Ok(other.clone()),
                }) {
                    Some(Ok(Object::Array(items))) => items
                        .iter()
                        .map(|item| Self::parse(document, item))
                        .collect::<Result<Vec<_>>>()?,
                    _ => return Err(Error::malformed("a stitching function without /Functions")),
                };
                Kind::Stitching {
                    functions,
                    bounds: reals(&dict, b"Bounds")?,
                    encode: reals(&dict, b"Encode")?,
                }
            }
            Some(4) => {
                let stream =
                    stream.ok_or(Error::malformed("a calculator function without a stream"))?;
                let program = parse_program(&document.decode_stream(&stream)?)?;
                Kind::Calculator { program }
            }
            Some(1) => {
                return Err(Error::Unsupported {
                    what: "type 1 (PDF 1.0 sampled) functions",
                })
            }
            _ => {
                return Err(Error::malformed(
                    "a /Function without a known /FunctionType",
                ))
            }
        };
        let domain = pairs(&dict, b"Domain")?;
        let range = pairs(&dict, b"Range")?;
        Ok(Self {
            domain,
            range,
            kind,
        })
    }

    /// Evaluate at `inputs`, clipping to `/Domain` and `/Range` (7.10.1).
    pub(crate) fn eval(&self, inputs: &[f64]) -> Vec<f64> {
        let mut values = Vec::with_capacity(self.domain.len());
        for (index, (low, high)) in self.domain.iter().enumerate() {
            let value: f64 = inputs.get(index).copied().unwrap_or(0.0);
            values.push(value.clamp(*low, *high));
        }
        let outputs = match &self.kind {
            Kind::Exponential { c0, c1, exponent } => {
                let x = values.first().copied().unwrap_or(0.0);
                let power = if *exponent == 0.0 {
                    1.0
                } else {
                    powf(x, *exponent)
                };
                c0.iter()
                    .zip(c1)
                    .map(|(c0, c1)| c0 + power * (c1 - c0))
                    .collect()
            }
            Kind::Stitching {
                functions,
                bounds,
                encode,
            } => {
                let x = values.first().copied().unwrap_or(0.0);
                let index = bounds.iter().filter(|bound| x >= **bound).count();
                let function = functions.get(index).or_else(|| functions.last());
                let Some(function) = function else {
                    return Vec::new();
                };
                let (low, high) = bounds.get(index.wrapping_sub(1)).copied().map_or(
                    (
                        self.domain.first().map_or(0.0, |d| d.0),
                        bounds.first().copied().unwrap_or(1.0),
                    ),
                    |low| (low, bounds.get(index).copied().unwrap_or(1.0)),
                );
                let target = encode
                    .get(index * 2)
                    .copied()
                    .zip(encode.get(index * 2 + 1).copied());
                let (from, to) = target.unwrap_or((0.0, 1.0));
                let span = if high > low { high - low } else { 1.0 };
                let mapped = from + (x - low) / span * (to - from);
                function.eval(&[mapped])
            }
            Kind::Calculator { program } => evaluate(program, &values).unwrap_or_default(),
            Kind::Sampled {
                sizes,
                bits,
                encode,
                decode,
                samples,
            } => sample(sizes, *bits, encode, decode, samples, &values),
        };
        if self.range.is_empty() {
            return outputs;
        }
        outputs
            .into_iter()
            .enumerate()
            .map(|(index, value)| match self.range.get(index) {
                Some((low, high)) => value.clamp(*low, *high),
                None => value,
            })
            .collect()
    }
}

/// Multilinear interpolation in the sample table (7.10.2).
fn sample(
    sizes: &[usize],
    bits: u8,
    encode: &[f64],
    decode: &[f64],
    samples: &[u32],
    values: &[f64],
) -> Vec<f64> {
    let outputs = decode.len().max(1) / 2;
    if sizes.is_empty() {
        return Vec::new();
    }
    // Encode each input into its own axis, then interpolate the corners.
    let mut positions = Vec::with_capacity(sizes.len());
    for (index, size) in sizes.iter().enumerate() {
        let (low, high) = encode
            .get(index * 2)
            .copied()
            .zip(encode.get(index * 2 + 1).copied())
            .unwrap_or((0.0, 1.0));
        let value = values.get(index).copied().unwrap_or(0.0);
        let position = low + value * (high - low);
        positions.push(position.clamp(0.0, (*size as f64).max(1.0)));
    }
    let max_bits = (1u32 << bits.min(31)) - 1;
    let mut result = Vec::with_capacity(outputs);
    for output in 0..outputs {
        // Linear interpolation along every axis, one axis at a time.
        let mut corners: Vec<(f64, f64)> = Vec::new();
        for (axis, size) in sizes.iter().enumerate() {
            let position = positions[axis];
            let low_index = (position.floor() as usize).min(size.saturating_sub(1));
            let high_index = (low_index + 1).min(size.saturating_sub(1));
            let fraction = position - position.floor();
            corners.push((low_index as f64, high_index as f64));
            let _ = fraction;
        }
        let mut accumulator = 0.0;
        // Enumerate every corner of the cell, weighted by the fractions.
        for corner in 0..(1usize << sizes.len()) {
            let mut weight = 1.0;
            let mut index = 0usize;
            let mut stride = 1usize;
            for (axis, size) in sizes.iter().enumerate() {
                let position = positions[axis];
                let low_index = (position.floor() as usize).min(size.saturating_sub(1));
                let high_index = (low_index + 1).min(size.saturating_sub(1));
                let fraction = (position - position.floor()).clamp(0.0, 1.0);
                let (selected, selected_weight) = if corner & (1 << axis) != 0 {
                    (high_index, fraction)
                } else {
                    (low_index, 1.0 - fraction)
                };
                weight *= selected_weight;
                index += selected * stride;
                stride *= (*size).max(1);
            }
            let sample_index = index * outputs + output;
            let raw = samples.get(sample_index).copied().unwrap_or(0);
            #[allow(clippy::cast_precision_loss)]
            let normalised = if max_bits == 0 {
                0.0
            } else {
                f64::from(raw) / f64::from(max_bits)
            };
            let (from, to) = decode
                .get(output * 2)
                .copied()
                .zip(decode.get(output * 2 + 1).copied())
                .unwrap_or((0.0, 1.0));
            accumulator += weight * (from + normalised * (to - from));
        }
        result.push(accumulator);
    }
    let _ = bits;
    result
}

/// The sample values of a sampled function, packed at `bits` per sample.
fn unpack_samples(document: &Document, stream: &Stream, bits: u8) -> Result<Vec<u32>> {
    let data = document.decode_stream(stream)?;
    if bits == 0 || bits > 16 {
        return Err(Error::malformed("a /BitsPerSample outside 1 to 16"));
    }
    let mut samples = Vec::new();
    let mut accumulator: u32 = 0;
    let mut held = 0u32;
    for byte in &data {
        accumulator = (accumulator << 8) | u32::from(*byte);
        held += 8;
        while held >= u32::from(bits) {
            let shift = held - u32::from(bits);
            samples.push((accumulator >> shift) & ((1u32 << bits) - 1));
            held -= u32::from(bits);
        }
    }
    Ok(samples)
}

/// Parse a calculator program into a flat token list with nested procs.
fn parse_program(data: &[u8]) -> Result<Vec<Token>> {
    let mut position = 0;
    let (program, _) = parse_block(data, &mut position, false)?;
    // A stream whose whole content is one braced block — `{ pop }` — is that
    // block's body: the braces are the writer's habit, not nesting.
    match program.as_slice() {
        [Token::Proc(body)] => Ok(body.clone()),
        _ => Ok(program),
    }
}

fn parse_block(data: &[u8], position: &mut usize, nested: bool) -> Result<(Vec<Token>, bool)> {
    let mut tokens = Vec::new();
    loop {
        while data
            .get(*position)
            .is_some_and(|byte| crate::function::is_space(*byte))
        {
            *position += 1;
        }
        let Some(&byte) = data.get(*position) else {
            if nested {
                return Err(Error::malformed(
                    "a calculator program without its closing brace",
                ));
            }
            return Ok((tokens, nested));
        };
        match byte {
            b'}' if nested => {
                *position += 1;
                return Ok((tokens, true));
            }
            b'{' => {
                *position += 1;
                let (inner, _) = parse_block(data, position, true)?;
                tokens.push(Token::Proc(inner));
            }
            b'%' => {
                while data.get(*position).is_some_and(|byte| *byte != b'\n') {
                    *position += 1;
                }
            }
            byte if byte.is_ascii_digit() || byte == b'-' || byte == b'+' || byte == b'.' => {
                let start = *position;
                *position += 1;
                while data.get(*position).is_some_and(|byte| {
                    byte.is_ascii_digit() || *byte == b'.' || *byte == b'e' || *byte == b'E'
                }) {
                    *position += 1;
                }
                let text = core::str::from_utf8(&data[start..*position])
                    .map_err(|_| Error::malformed("a calculator number"))?;
                let value: f64 = text
                    .parse()
                    .map_err(|_| Error::malformed("a calculator number"))?;
                tokens.push(Token::Number(value));
            }
            _ => {
                let start = *position;
                while data.get(*position).is_some_and(|byte| {
                    !crate::function::is_space(*byte) && !matches!(*byte, b'{' | b'}' | b'%')
                }) {
                    *position += 1;
                }
                let name = core::str::from_utf8(&data[start..*position])
                    .map_err(|_| Error::malformed("a calculator operator"))?;
                let operator = Op::from_name(name).ok_or(Error::Unsupported {
                    what: "a calculator operator outside Table 42",
                })?;
                tokens.push(Token::Operator(operator));
            }
        }
    }
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\x00')
}

/// A value on the calculator stack: a number, or a procedure for `if`/`ifelse`.
#[derive(Debug, Clone)]
enum Value {
    Number(f64),
    Proc(Vec<Token>),
}

impl Value {
    fn number(self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(value),
            Self::Proc(_) => None,
        }
    }
}

/// Run a calculator program over `inputs` (7.10.5).
fn evaluate(program: &[Token], inputs: &[f64]) -> Option<Vec<f64>> {
    let mut stack: Vec<Value> = inputs.iter().copied().map(Value::Number).collect();
    let mut steps = 0;
    run(program, &mut stack, &mut steps)?;
    let mut out = Vec::with_capacity(stack.len());
    for value in stack {
        out.push(value.number()?);
    }
    Some(out)
}

fn run(tokens: &[Token], stack: &mut Vec<Value>, steps: &mut usize) -> Option<()> {
    for token in tokens {
        *steps += 1;
        if *steps > MAX_STEPS {
            return None;
        }
        match token {
            Token::Number(value) => stack.push(Value::Number(*value)),
            Token::Proc(body) => stack.push(Value::Proc(body.clone())),
            Token::Operator(operator) => apply(*operator, stack, steps)?,
        }
        if stack.len() > MAX_STACK {
            return None;
        }
    }
    Some(())
}

fn pop(stack: &mut Vec<Value>) -> Option<f64> {
    stack.pop()?.number()
}

fn flag(stack: &mut Vec<Value>) -> Option<bool> {
    pop(stack).map(|value| value != 0.0)
}

fn pop_proc(stack: &mut Vec<Value>) -> Option<Vec<Token>> {
    match stack.pop()? {
        Value::Proc(body) => Some(body),
        Value::Number(_) => None,
    }
}

fn apply(operator: Op, stack: &mut Vec<Value>, steps: &mut usize) -> Option<()> {
    match operator {
        Op::Abs => {
            let value = pop(stack)?;
            stack.push(Value::Number(abs(value)));
        }
        Op::Add => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            stack.push(Value::Number(a + b));
        }
        Op::Sub => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            stack.push(Value::Number(a - b));
        }
        Op::Mul => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            stack.push(Value::Number(a * b));
        }
        Op::Div => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            if b == 0.0 {
                return None;
            }
            stack.push(Value::Number(a / b));
        }
        Op::Idiv => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            if b == 0.0 {
                return None;
            }
            stack.push(Value::Number((a / b).trunc()));
        }
        Op::Mod => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            if b == 0.0 {
                return None;
            }
            stack.push(Value::Number(a % b));
        }
        Op::Neg => {
            let value = pop(stack)?;
            stack.push(Value::Number(-value));
        }
        Op::Ceiling => {
            let value = pop(stack)?;
            stack.push(Value::Number(value.ceil()));
        }
        Op::Floor => {
            let value = pop(stack)?;
            stack.push(Value::Number(value.floor()));
        }
        Op::Round => {
            let value = pop(stack)?;
            stack.push(Value::Number(value.round()));
        }
        Op::Truncate => {
            let value = pop(stack)?;
            stack.push(Value::Number(value.trunc()));
        }
        Op::Sqrt => {
            let value = pop(stack)?;
            if value < 0.0 {
                return None;
            }
            stack.push(Value::Number(sqrt(value)));
        }
        Op::Sin => {
            let value = pop(stack)?;
            stack.push(Value::Number(sin(value)));
        }
        Op::Cos => {
            let value = pop(stack)?;
            stack.push(Value::Number(cos(value)));
        }
        Op::Atan => {
            let denominator = pop(stack)?;
            let numerator = pop(stack)?;
            stack.push(Value::Number(atan2(numerator, denominator)));
        }
        Op::Exp => {
            let exponent = pop(stack)?;
            let base = pop(stack)?;
            stack.push(Value::Number(powf(base, exponent)));
        }
        Op::Ln => {
            let value = pop(stack)?;
            if value <= 0.0 {
                return None;
            }
            stack.push(Value::Number(ln(value)));
        }
        Op::Log => {
            let value = pop(stack)?;
            if value <= 0.0 {
                return None;
            }
            stack.push(Value::Number(ln(value) / ln(10.0)));
        }
        Op::Cvi => {
            let value = pop(stack)?;
            stack.push(Value::Number(value.trunc()));
        }
        Op::Cvr => {}
        Op::True => stack.push(Value::Number(1.0)),
        Op::False => stack.push(Value::Number(0.0)),
        Op::Eq => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            stack.push(Value::Number(if a == b { 1.0 } else { 0.0 }));
        }
        Op::Ne => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            stack.push(Value::Number(if a != b { 1.0 } else { 0.0 }));
        }
        Op::Lt => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            stack.push(Value::Number(if a < b { 1.0 } else { 0.0 }));
        }
        Op::Le => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            stack.push(Value::Number(if a <= b { 1.0 } else { 0.0 }));
        }
        Op::Gt => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            stack.push(Value::Number(if a > b { 1.0 } else { 0.0 }));
        }
        Op::Ge => {
            let b = pop(stack)?;
            let a = pop(stack)?;
            stack.push(Value::Number(if a >= b { 1.0 } else { 0.0 }));
        }
        Op::And => {
            let b = flag(stack)?;
            let a = flag(stack)?;
            stack.push(Value::Number(if a && b { 1.0 } else { 0.0 }));
        }
        Op::Or => {
            let b = flag(stack)?;
            let a = flag(stack)?;
            stack.push(Value::Number(if a || b { 1.0 } else { 0.0 }));
        }
        Op::Xor => {
            let b = flag(stack)?;
            let a = flag(stack)?;
            stack.push(Value::Number(if a ^ b { 1.0 } else { 0.0 }));
        }
        Op::Not => {
            let value = pop(stack)?;
            stack.push(Value::Number(if value == 0.0 { 1.0 } else { 0.0 }));
        }
        Op::BitShift => {
            let shift = pop(stack)?;
            let value = pop(stack)?;
            #[allow(clippy::cast_possible_truncation)]
            let (value, shift) = (value.trunc() as i64, shift.trunc() as i32);
            let result = if shift >= 0 {
                value.checked_shl(shift.unsigned_abs()).unwrap_or(0)
            } else {
                value.checked_shr(shift.unsigned_abs()).unwrap_or(0)
            };
            #[allow(clippy::cast_precision_loss)]
            stack.push(Value::Number(result as f64));
        }
        Op::Dup => {
            let value = stack.last()?.clone();
            stack.push(value);
        }
        Op::Pop => {
            stack.pop()?;
        }
        Op::Exch => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(b);
            stack.push(a);
        }
        Op::Copy => {
            let count = pop(stack)?;
            if count < 0.0 {
                return None;
            }
            let count = count.trunc() as usize;
            if count > stack.len() {
                return None;
            }
            let from = stack.len() - count;
            let copied: Vec<Value> = stack[from..].to_vec();
            stack.extend(copied);
        }
        Op::Index => {
            let index = pop(stack)?;
            if index < 0.0 {
                return None;
            }
            let index = index.trunc() as usize;
            if index >= stack.len() {
                return None;
            }
            let value = stack[stack.len() - 1 - index].clone();
            stack.push(value);
        }
        Op::Roll => {
            let count = pop(stack)?;
            let shift = pop(stack)?;
            if count < 0.0 || count as usize > stack.len() {
                return None;
            }
            let count = count.trunc() as usize;
            if count == 0 {
                return Some(());
            }
            let start = stack.len() - count;
            let span = &mut stack[start..];
            let shift = (shift.trunc() as i64).rem_euclid(count as i64) as usize;
            span.rotate_right(shift);
        }
        Op::If => {
            let body = pop_proc(stack)?;
            let condition = pop(stack)? != 0.0;
            if condition {
                run(&body, stack, steps)?;
            }
        }
        Op::Ifelse => {
            let second = pop_proc(stack)?;
            let first = pop_proc(stack)?;
            let condition = pop(stack)? != 0.0;
            run(if condition { &first } else { &second }, stack, steps)?;
        }
    }
    Some(())
}
// The maths functions live in `std`, and this crate is `std`-only; these thin
// wrappers keep the interpreter's operators readable.
fn abs(value: f64) -> f64 {
    value.abs()
}
fn sqrt(value: f64) -> f64 {
    value.sqrt()
}
fn sin(value: f64) -> f64 {
    value.sin()
}
fn cos(value: f64) -> f64 {
    value.cos()
}
fn atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
fn ln(value: f64) -> f64 {
    value.ln()
}
fn powf(base: f64, exponent: f64) -> f64 {
    base.powf(exponent)
}

fn integers(dict: &Dictionary, key: &[u8]) -> Result<Vec<usize>> {
    match dict.get(key) {
        Some(Object::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_integer()
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or(Error::malformed("a /Size that is not a positive integer"))
            })
            .collect(),
        _ => Err(Error::malformed("a sampled function without /Size")),
    }
}

fn reals(dict: &Dictionary, key: &[u8]) -> Result<Vec<f64>> {
    match dict.get(key) {
        Some(Object::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_real()
                    .ok_or(Error::malformed("a /Function array of numbers"))
            })
            .collect(),
        _ => Ok(Vec::new()),
    }
}

fn pairs(dict: &Dictionary, key: &[u8]) -> Result<Vec<(f64, f64)>> {
    let values = reals(dict, key)?;
    Ok(values
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::Function;
    use pdf2::{Document, Object};

    /// A PDF holding the given objects (number, body bytes).
    fn document(objects: &[(u32, Vec<u8>)]) -> Document {
        let mut out = b"%PDF-2.0\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (number, body) in objects {
            offsets.push((*number, out.len()));
            out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        // One subsection per object, so sparse numbering is legal.
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 1\n0000000000 65535 f \n");
        for (number, offset) in &offsets {
            out.extend_from_slice(format!("{number} 1\n{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                offsets.len() + 1
            )
            .as_bytes(),
        );
        Document::open(out).expect("document")
    }

    fn parse(objects: &[(u32, Vec<u8>)], number: u32) -> Function {
        let document = document(objects);
        Function::parse(
            &document,
            &Object::Reference(pdf2::ObjectId::new(number, 0)),
        )
        .expect("parse")
    }

    #[test]
    fn an_exponential_function_follows_its_formula() {
        let function = parse(
            &[object(
                7,
                "<< /FunctionType 2 /Domain [0 1] /Range [0 1] /C0 [0] /C1 [1] /N 2 >>",
            )],
            7,
        );
        assert!((function.eval(&[0.0])[0] - 0.0).abs() < 1e-9);
        assert!((function.eval(&[0.5])[0] - 0.25).abs() < 1e-9);
        assert!((function.eval(&[1.0])[0] - 1.0).abs() < 1e-9);
        // Outside the domain the input is clipped, not extrapolated.
        assert!((function.eval(&[2.0])[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_stitching_function_picks_by_its_bounds() {
        let function = parse(
            &[
                object(
                    7,
                    "<< /FunctionType 2 /Domain [0 1] /Range [0 1] /C0 [0] /C1 [1] /N 1 >>",
                ),
                object(
                    8,
                    "<< /FunctionType 2 /Domain [0 1] /Range [0 20] /C0 [10] /C1 [20] /N 1 >>",
                ),
                object(
                    9,
                    "<< /FunctionType 3 /Domain [0 1] /Range [0 30] /Functions [7 0 R 8 0 R] \
                     /Bounds [0.5] /Encode [0 1 0 1] >>",
                ),
            ],
            9,
        );
        assert!((function.eval(&[0.25])[0] - 0.5).abs() < 1e-9, "first half");
        assert!(
            (function.eval(&[0.75])[0] - 15.0).abs() < 1e-9,
            "second half: {}",
            function.eval(&[0.75])[0]
        );
    }

    /// A stream object with `/Length` computed from its body.
    fn stream(number: u32, dict: &str, body: &[u8]) -> (u32, Vec<u8>) {
        let mut out = format!("<< {dict} /Length {} >>\nstream\n", body.len()).into_bytes();
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendstream");
        (number, out)
    }

    fn object(number: u32, body: &str) -> (u32, Vec<u8>) {
        (number, body.as_bytes().to_vec())
    }

    #[test]
    fn a_calculator_program_runs_its_operators() {
        // `{ add 2 div }` is the corpus's own averaging program.
        let (number, body) = stream(
            7,
            "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1]",
            b"{ add 2 div }",
        );
        let function = parse(&[(number, body)], number);
        assert!((function.eval(&[0.2, 0.6])[0] - 0.4).abs() < 1e-9);
    }

    #[test]
    fn a_calculator_program_can_branch() {
        // `{ 5 gt { 1 } { 0 } ifelse }`: one above five, zero otherwise.
        let (number, body) = stream(
            7,
            "/FunctionType 4 /Domain [0 10] /Range [0 1]",
            b"{ 5 gt { 1 } { 0 } ifelse }",
        );
        let function = parse(&[(number, body)], number);
        assert!((function.eval(&[7.0])[0] - 1.0).abs() < 1e-9);
        assert!((function.eval(&[3.0])[0] - 0.0).abs() < 1e-9);
    }

    #[test]
    fn a_sampled_function_interpolates_its_table() {
        // Two samples, 0 and 255, over [0 1]: the midpoint is half way.
        let (number, body) = stream(
            7,
            "/FunctionType 0 /Domain [0 1] /Range [0 1] /Size [2] /BitsPerSample 8 /Encode [0 1] /Decode [0 1]",
            &[0x00, 0xff],
        );
        let function = parse(&[(number, body)], number);
        assert!((function.eval(&[0.0])[0] - 0.0).abs() < 1e-3);
        assert!((function.eval(&[0.5])[0] - 0.5).abs() < 1e-3);
        assert!((function.eval(&[1.0])[0] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_corpus_functions_all_parse_and_evaluate() {
        // The real prize: every `/Function` in the two shading pages parses, and
        // evaluating it produces finite numbers in its range.
        for path in [
            "corpus/tier-a/function_based_shading.pdf",
            "corpus/tier-a/function_based_shading_cmyk.pdf",
            "corpus/tier-a/axial_shading_many_inputs.pdf",
            "corpus/tier-a/ShowText-ShadingPattern.pdf",
            "corpus/tier-a/alphatrans.pdf",
        ] {
            let full = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
            let bytes = std::fs::read(format!("{full}/{path}")).expect("corpus file");
            let document = Document::open(bytes).expect("open");
            let size = document
                .trailer()
                .get(b"Size")
                .and_then(Object::as_integer)
                .unwrap_or(64);
            let mut parsed = 0;
            for number in 1..=u32::try_from(size).unwrap_or(64) {
                if !document.contains_object(number) {
                    continue;
                }
                let Ok(object) = document.get_object_by_number(number) else {
                    continue;
                };
                let dict = match &object {
                    Object::Dictionary(dict) => dict.clone(),
                    Object::Stream(stream) => stream.dict().clone(),
                    _ => continue,
                };
                if dict.get(b"FunctionType").is_none() {
                    continue;
                }
                let function = Function::parse(
                    &document,
                    &Object::Reference(pdf2::ObjectId::new(number, 0)),
                )
                .unwrap_or_else(|error| panic!("{path} object {number}: {error}"));
                for step in 0..=10 {
                    #[allow(clippy::cast_precision_loss)]
                    let x = f64::from(step) / 10.0;
                    for value in function.eval(&[x]) {
                        assert!(value.is_finite(), "{path} object {number} at {x}");
                    }
                }
                parsed += 1;
            }
            assert!(parsed > 0, "{path} has functions");
        }
    }

    #[test]
    fn a_deprecated_type_1_function_is_refused_by_name() {
        let document = document(&[object(
            7,
            "<< /FunctionType 1 /Domain [0 1] /Range [0 1] >>",
        )]);
        let error = Function::parse(&document, &Object::Reference(pdf2::ObjectId::new(7, 0)))
            .expect_err("type 1");
        assert!(error.to_string().contains("type 1"), "{error}");
    }
}
