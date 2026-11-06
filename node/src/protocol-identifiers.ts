const utf8Encoder = new TextEncoder();
const CONTROL_CHARACTERS = /[\u0000-\u001f\u007f-\u009f]/u;
const SCHEMA_ID = /^[A-Za-z0-9._:-]{1,128}$/;
const ACTION_CODE = /^[A-Za-z0-9._:-]{1,128}$/;

export function isValidProtocolKey(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && utf8Encoder.encode(value).byteLength <= 256
    && value.trim() === value
    && !CONTROL_CHARACTERS.test(value);
}

export function isValidActionSchemaId(value: unknown): value is string {
  return typeof value === "string" && SCHEMA_ID.test(value);
}

export function isValidActionCode(value: unknown): value is string {
  return typeof value === "string" && ACTION_CODE.test(value);
}
