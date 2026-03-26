const wasmStatus = document.querySelector("#wasm-status");
const decoderStatus = document.querySelector("#decoder-status");
const elfFileInput = document.querySelector("#elf-file");
const payloadsInput = document.querySelector("#payloads");
const decodeButton = document.querySelector("#decode");
const createDecoderButton = document.querySelector("#create-decoder");
const clearButton = document.querySelector("#reset-output");
const logOutput = document.querySelector("#log-output");
const warningOutput = document.querySelector("#warning-output");

let wasmModule = null;
let decoder = null;

loadWasmModule();

createDecoderButton.addEventListener("click", async () => {
  if (!wasmModule) {
    setStatus(wasmStatus, "Build the WASM package first. See this example's README.", true);
    return;
  }

  const elfFile = elfFileInput.files?.[0];
  if (!elfFile) {
    setStatus(decoderStatus, "Choose a firmware ELF first.", true);
    return;
  }

  try {
    const elfBytes = new Uint8Array(await elfFile.arrayBuffer());
    decoder = new wasmModule.DefmtDecoder(elfBytes);
    setStatus(decoderStatus, `Decoder ready for ${elfFile.name}.`, false);
  } catch (error) {
    decoder = null;
    setStatus(decoderStatus, formatError(error), true);
  }
});

decodeButton.addEventListener("click", () => {
  if (!decoder) {
    setStatus(decoderStatus, "Create the decoder before decoding payloads.", true);
    return;
  }

  try {
    const chunks = parsePayloadLines(payloadsInput.value);
    const decodedLines = [];
    const warnings = [];

    for (const chunk of chunks) {
      const result = decoder.decodeChunk(chunk);
      decodedLines.push(...result.lines);
      warnings.push(...result.warnings);
    }

    logOutput.textContent =
      decodedLines.length > 0 ? decodedLines.join("\n") : "No complete log lines yet.";
    warningOutput.textContent = warnings.length > 0 ? warnings.join("\n") : "No warnings.";
  } catch (error) {
    warningOutput.textContent = formatError(error);
  }
});

clearButton.addEventListener("click", () => {
  logOutput.textContent = "Decoded lines will appear here.";
  warningOutput.textContent = "Recoverable warnings will appear here.";
});

async function loadWasmModule() {
  try {
    const module = await import("./pkg/defmt_mqtt_decoder.js");
    await module.default();
    wasmModule = module;
    setStatus(wasmStatus, "WASM package loaded.", false);
  } catch (error) {
    setStatus(
      wasmStatus,
      `Could not load ./pkg/defmt_mqtt_decoder.js. Build it first. Details: ${formatError(error)}`,
      true,
    );
  }
}

function parsePayloadLines(text) {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map(parseHexBytes);
}

function parseHexBytes(line) {
  const compact = line.replaceAll(/\s+/g, "");
  if (compact.length === 0) {
    return new Uint8Array();
  }

  if (compact.length % 2 !== 0) {
    throw new Error(`Hex payload must contain an even number of characters: ${line}`);
  }

  if (!/^[0-9a-fA-F]+$/.test(compact)) {
    throw new Error(`Hex payload contains non-hex characters: ${line}`);
  }

  const bytes = new Uint8Array(compact.length / 2);

  for (let index = 0; index < compact.length; index += 2) {
    bytes[index / 2] = Number.parseInt(compact.slice(index, index + 2), 16);
  }

  return bytes;
}

function setStatus(element, message, isError) {
  element.textContent = message;
  element.style.color = isError ? "#a12f11" : "#365840";
}

function formatError(error) {
  if (error instanceof Error) {
    return error.message;
  }

  return String(error);
}
