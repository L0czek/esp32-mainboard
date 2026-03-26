use js_sys::Array;
use wasm_bindgen::prelude::*;

use crate::decoder::{DecodedChunk, DefmtStreamDecoder};

/// WASM-facing incremental decoder for raw `defmt` MQTT payload bytes.
#[wasm_bindgen(js_name = DefmtDecoder)]
pub struct WasmDefmtDecoder {
    inner: DefmtStreamDecoder,
}

/// WASM-facing result for one decoded payload chunk.
#[wasm_bindgen(js_name = DecodedChunk)]
pub struct WasmDecodedChunk {
    lines: Vec<String>,
    warnings: Vec<String>,
}

#[wasm_bindgen(js_class = DefmtDecoder)]
impl WasmDefmtDecoder {
    /// Creates a decoder from the firmware ELF bytes used to encode the MQTT stream.
    #[wasm_bindgen(constructor)]
    pub fn new(elf_bytes: &[u8]) -> Result<Self, JsError> {
        let inner = DefmtStreamDecoder::from_elf_bytes(elf_bytes)
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(Self { inner })
    }

    /// Decodes one MQTT payload chunk and returns any completed lines.
    #[wasm_bindgen(js_name = decodeChunk)]
    pub fn decode_chunk(&mut self, payload: &[u8]) -> Result<WasmDecodedChunk, JsError> {
        let chunk = self
            .inner
            .decode_chunk(payload)
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(chunk.into())
    }
}

#[wasm_bindgen(js_class = DecodedChunk)]
impl WasmDecodedChunk {
    /// Returns formatted log lines completed by the last payload.
    #[wasm_bindgen(getter)]
    pub fn lines(&self) -> Array {
        strings_to_array(&self.lines)
    }

    /// Returns recoverable warnings emitted while skipping malformed frames.
    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> Array {
        strings_to_array(&self.warnings)
    }
}

impl From<DecodedChunk> for WasmDecodedChunk {
    fn from(value: DecodedChunk) -> Self {
        Self {
            lines: value.lines,
            warnings: value.warnings,
        }
    }
}

fn strings_to_array(values: &[String]) -> Array {
    let array = Array::new();

    for value in values {
        array.push(&JsValue::from_str(value));
    }

    array
}
